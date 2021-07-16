use bincode;
use serde::{Deserialize, Serialize};
use std::{
    io::Write,
    net::SocketAddr,
    sync::{
        atomic::{AtomicBool, AtomicU8, AtomicUsize, Ordering},
        Arc, Weak,
    },
    time::Duration,
};
use tokio::{net::UdpSocket, task::JoinHandle};

pub type Identity = u32;
pub type Identity2 = u32;
pub type IResult<T> = Result<T, anyhow::Error>;

#[macro_export]
macro_rules! println_lined{
    ($($tt:tt)*)=>{
        println!("[{}:{}] {}",file!(),line!(),format!($($tt)*));
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub enum Operate {
    Open,
    Opened(Identity),
    HelloTo(Identity),
    ConnectToMe(Identity2, Identity2), //第一个是信息管道，第二个是游戏管道
    BridgeTo(Identity2),
    //Bridged(Identity2),
    OperationFailed,
    HeartBeat,
}

pub struct CommunicatePacket;

pub struct MCPEInfo<'a> {
    splits: Vec<&'a [u8]>,
    pub world_name: String,
    pub game_port: u16,
}

pub struct BridgeClient {
    udp: Weak<UdpSocket>,
    saddr: SocketAddr,
    baddr: SocketAddr,
    runtime: JoinHandle<IResult<()>>,
    gc_timer: Arc<AtomicU8>,
}

struct RingBuffer {
    buffer: Box<[u8]>,
    //writing:AtomicBool,
    start: AtomicUsize,
    end: AtomicUsize,
}

impl CommunicatePacket {
    pub const CONNECT: u8 = 0;
    //pub const ACK: u8 = 1;
    pub const DATA: u8 = 2;
    pub const HEARTBEAT: u8 = 3;
    //pub const RESET: u8 = 4;
}

impl Operate {
    #[inline]
    pub fn deserialize(b: &[u8]) -> Option<Operate> {
        match bincode::deserialize(b) {
            Ok(v) => Some(v),
            Err(err) => {
                dbg!(err);
                None
            }
        }
    }

    #[inline]
    pub fn serialize(&self) -> Vec<u8> {
        bincode::serialize(self).unwrap()
    }
}

impl RingBuffer {
    fn new(buf_size: usize) -> Self {
        Self::with_capacity(0)
    }

    fn with_capacity(buf_size: usize) -> Self {
        RingBuffer {
            buffer: vec![0u8; buf_size].into_boxed_slice(),
            start: AtomicUsize::new(0),
            end: AtomicUsize::new(0),
        }
    }

    #[inline]
    fn get_bound(&self) -> (usize, usize) {
        (
            self.start.load(Ordering::Relaxed),
            self.end.load(Ordering::Relaxed),
        )
    }

    fn write_exact(&self, buf: &[u8]) -> usize {
        let (start, mut end) = self.get_bound();
        //安全：已经保证不会写到读区域
        //强制将不可变引用改成可变引用
        let buffer = unsafe {
            (&*self.buffer as *const [u8] as *mut [u8])
                .as_mut()
                .unwrap()
        };
        if start < end {
            //数据没分片，那么空区就要分片
            let size = (&mut buffer[end..]).write(buf).unwrap();
            if size < buf.len() {
                //数据还没塞完
                let len = (&mut buffer[..start]).write(&buf[size..]).unwrap();
                self.end.store(len, Ordering::Relaxed);
                size + len
            } else {
                //一个空区塞得下
                self.end.store(end + size, Ordering::Relaxed);
                size
            }
        } else {
            //数据分片，空区就无需分片
            let size = (&mut buffer[end..start]).write(buf).unwrap();
            self.end.store(end + size, Ordering::Relaxed);
            size
        }
    }

    fn read_exact(&self, buf: &mut [u8]) -> Option<usize> {
        let (mut start, end) = self.get_bound();
        if start <= end {
            //数据没有断开
            let read = (&mut buf[..]).write(&self.buffer[start..end]).unwrap();
            self.start.store(start + read, Ordering::Relaxed);
            Some(read)
        } else {
            //数据断开了
            let r1 = (&mut buf[..]).write(&self.buffer[start..]).unwrap();
            start += r1;
            let r2 = (&mut buf[r1..]).write(&self.buffer[..end]).unwrap();
            start += r2;

            self.start.store(start, Ordering::Relaxed);
            Some(r1 + r2)
        }
    }

    fn free_cap(&self) -> usize {
        let (start, end) = self.get_bound();
        if start < end {
            self.buffer.len() - (end - start)
        } else {
            start - end
        }
    }
}

impl<'a> MCPEInfo<'a> {
    pub fn deserialize(input: &'a [u8]) -> Option<Self> {
        let mut counts = 0usize;
        const SPLIT_INDEX: [usize; 5] = [6, 7, 9, 10, 11]; //切割位点

        let sp: Vec<&'a [u8]> = input
            .splitn(6, |code| {
                if *code == b';' {
                    if SPLIT_INDEX.contains(&counts) {
                        counts += 1;
                        return true;
                    } else {
                        counts += 1;
                        return false;
                    }
                } else {
                    return false;
                }
            })
            .collect();

        if sp.len() != 6 {
            println_lined!("mcpe info deserialize error, buffer: {:?}", input);
            return None;
        }

        let world_name = String::from_utf8_lossy(&sp[1]).to_string();
        let game_port = String::from_utf8_lossy(&sp[5]).parse().unwrap_or(0);

        return Some(MCPEInfo {
            splits: sp,
            world_name,
            game_port,
        });
    }

    pub fn serialize(&self) -> Vec<u8> {
        let mut slices = Vec::with_capacity(6);
        slices.extend_from_slice(&self.splits);

        slices[1] = self.world_name.as_bytes();
        let game_port1 = (self.game_port - 1).to_string();
        let game_port2 = self.game_port.to_string();
        slices[3] = game_port1.as_bytes();
        slices[4] = game_port2.as_bytes();

        return slices.join(&b";"[..]);
    }
}

impl BridgeClient {
    pub async fn connect(
        cid: Identity2,
        saddr: SocketAddr, //服务器的地址
        baddr: SocketAddr, //绑定的地址
        name: &'static str,
    ) -> IResult<Self> {
        let udp = Arc::new(UdpSocket::bind(baddr).await?);
        let gc_timer = Arc::new(AtomicU8::new(0)); //垃圾回收计时器
        let packet = {
            let mut packet = [0u8; 5];
            packet[0] = CommunicatePacket::CONNECT;
            (&mut packet[1..5]).write(&cid.to_be_bytes()).unwrap();
            packet
        };

        //间隔发包
        let pinger = {
            let udp = udp.clone();
            tokio::spawn(async move {
                loop {
                    if let Err(err) = udp.send_to(&packet, &saddr).await {
                        println_lined!("Unexpected error:{}", err);
                    } else {
                        println_lined!("{}已发包: {}", name, cid);
                    }
                    tokio::time::sleep(Duration::from_millis(1000)).await;
                }
            })
        };

        //监听是否连接成功
        {
            let udp = udp.clone();
            let saddr = saddr.clone();
            let result = tokio::time::timeout(Duration::from_secs(10), async move {
                let mut buf = [0u8; 1500];
                loop {
                    let (len, raddr) = udp.recv_from(&mut buf).await?;
                    if raddr == saddr && buf[..len] == packet {
                        break IResult::Ok(());
                    } else {
                        println!("检验错误");
                    }
                }
            })
            .await;
            pinger.abort();
            match result {
                Ok(n) => {
                    n?;
                }
                Err(_) => {
                    Err(anyhow::anyhow!(
                        "No response from the server after 10s. Abort."
                    ))?;
                }
            }
        }

        println!("`{}`连接成功", name);

        //启动心跳包运行时和垃圾清理运行时
        let runtime: JoinHandle<IResult<()>> = {
            let udp = udp.clone();
            let gc_timer = gc_timer.clone();
            let saddr = saddr.clone();
            tokio::spawn(async move {
                let packet = [CommunicatePacket::HEARTBEAT];
                let mut interval = tokio::time::interval(Duration::from_secs(2));
                loop {
                    interval.tick().await;
                    udp.send_to(&packet, &saddr).await?;
                    continue;

                    let time = gc_timer.load(Ordering::Relaxed) + 1;

                    //静止时间大于7s开始发送心跳包
                    if time > 7 {
                        udp.send_to(&packet, &saddr).await?;
                    }

                    //静止时间大于15s销毁（释放计数引用）
                    if time > 15 {
                        break Ok(());
                    }

                    gc_timer.store(time, Ordering::Relaxed);
                }
            })
        };

        Ok(BridgeClient {
            udp: Arc::downgrade(&udp),
            saddr,
            baddr: udp.local_addr()?,
            runtime,
            gc_timer,
        })
    }

    pub async fn recv_from(&self, b: &mut [u8]) -> IResult<(usize, SocketAddr)> {
        match self.udp.upgrade() {
            Some(arc) => loop {
                let (len, raddr) = arc.recv_from(b).await?;
                self.gc_timer.store(0, Ordering::Relaxed);

                if b[0] == CommunicatePacket::DATA {
                    println!("RECV {:?}", &b[1..len]);
                    b.copy_within(1..len, 0);
                    break Ok((len - 1, raddr));
                }
            },
            None => Err(anyhow::anyhow!("The Arc has been dropped")),
        }
    }

    pub async fn send_to(&self, b: &[u8], d: &SocketAddr) -> IResult<()> {
        match self.udp.upgrade() {
            Some(arc) => {
                arc.send_to(b, d).await?;
                println!("SEND");
                Ok(())
            }
            None => Err(anyhow::anyhow!("The Arc has been dropped")),
        }
    }

    pub async fn alive(&self) -> bool {
        self.udp.upgrade().is_some()
    }

    #[inline]
    pub fn saddr(&self) -> &SocketAddr {
        &self.saddr
    }

    #[inline]
    pub fn baddr(&self) -> &SocketAddr {
        &self.baddr
    }

    /*pub async fn transpond_from(&self,from:&SocketAddr,b:&[u8])->Option<()>{
    if *from==self.saddr{
    self.send_to(b,&self.laddr).await?;
    }else if *from==self.laddr{
    self.send_to(b,&self.saddr).await?;
    }
    Some(())
    }*/
}

impl Drop for BridgeClient {
    fn drop(&mut self) {
        self.runtime.abort();
    }
}

pub async fn log_i_result<F>(name: &str, future: F) -> IResult<()>
where
    F: std::future::Future<Output = IResult<()>>,
{
    let result = future.await;
    if let Err(ref err) = result {
        println!("`{}`已断开。因为：{}", name, err);
    }
    result
}
