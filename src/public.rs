use bincode;
use serde::{Deserialize, Serialize};
use std::{
    io::Write,
    net::{IpAddr,SocketAddr},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};
use tokio::{net::UdpSocket, task::JoinHandle};
//use crate::ring_buffer;

pub type Identity = u32;
pub type Identity2 = u32;
pub type IResult<T> = Result<T, anyhow::Error>;

#[macro_export]
macro_rules! println_lined{
    ($($tt:tt)*)=>{
        println!("[{}:{}] {}",file!(),line!(),format!($($tt)*));
    }
}

lazy_static!{
    pub static ref LOCALADDR:IpAddr = {
        let foo=||{
            let udp=std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
            udp.connect("8.8.8.8:80").ok()?;
            Some(udp.local_addr().ok()?.ip())
        };

        match foo(){
            None=>{
                println_lined!("不能识别内网IP。");
                IpAddr::from([127,0,0,1])
            }
            Some(l)=>{
                println!("已识别内网IP：{}\n",l.to_string());
                l
            }
        }
    };
    pub static ref LOCALHOST:IpAddr=IpAddr::from([127,0,0,1]);
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

#[derive(Debug)]
pub struct MCPEInfo<'a> {
    splits: Vec<&'a [u8]>,
    pub world_name: String,
    pub game_port: u16,
}

pub struct BridgeClient {
    udp: Arc<UdpSocket>,
    saddr: SocketAddr,
    baddr: SocketAddr,
    runtimes: [JoinHandle<()>; 1],
    name: &'static str, //buffer:ring_buffer::Reader
    alive: Arc<AtomicBool>,
}

impl CommunicatePacket {
    pub const CONNECT: u8 = 0;
    pub const ACK: u8 = 1;
    pub const DATA: u8 = 2;
    pub const HEARTBEAT: u8 = 3;
    pub const CLOSE: u8 = 4;
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

impl<'a> MCPEInfo<'a> {
    pub fn deserialize(input: &'a [u8]) -> Option<Self> {
        let mut counts = 0usize;
        const SPLIT_INDEX: [usize; 5] = [7, 8, 10, 11, 12]; //切割位点

        let sp: Vec<&'a [u8]> = input
            .splitn(6, |code| {
                if *code == b';' {
                    counts += 1;
                    return SPLIT_INDEX.contains(&counts);
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
        let game_port = String::from_utf8_lossy(&sp[3]).parse().unwrap_or(0);

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
        let game_port1 = self.game_port.to_string();
        let game_port2 = (self.game_port + 1).to_string();
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
        let alive = Arc::new(AtomicBool::new(true));

        //间隔发包
        let pinger = {
            let udp = udp.clone();
            tokio::spawn(async move {
                let packet = {
                    let mut packet = [0u8; 5];
                    packet[0] = CommunicatePacket::CONNECT;
                    (&mut packet[1..5]).write(&cid.to_be_bytes()).unwrap();
                    packet
                };

                loop {
                    if let Err(err) = udp.send_to(&packet, &saddr).await {
                        println_lined!("Unexpected error:{}", err);
                    } else {
                        println_lined!("{}已发包: {}", name, cid);
                    }
                    tokio::time::sleep(Duration::from_millis(500)).await;
                }
            })
        };

        //监听是否连接成功
        {
            let udp = udp.clone();
            let saddr = saddr.clone();
            let cid_bytes = cid.to_be_bytes();

            let result = tokio::time::timeout(Duration::from_secs(10), async move {
                let mut buf = [0u8; 1500];
                loop {
                    let (len, raddr) = udp.recv_from(&mut buf).await?;
                    if len == 5
                        && raddr == saddr
                        && buf[0] == CommunicatePacket::ACK
                        && buf[1..5] == cid_bytes
                    {
                        break IResult::Ok(());
                    } else {
                        println!("检验错误 {:?}", &buf[..len]);
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

        //启动心跳包运行时
        let pack_sender = {
            let udp = udp.clone();
            let saddr = saddr.clone();
            let alive = alive.clone();
            tokio::spawn(async move {
                const PACKET: [u8; 1] = [CommunicatePacket::HEARTBEAT];
                let mut interval = tokio::time::interval(Duration::from_secs(2));
                loop {
                    interval.tick().await;
                    match udp.send_to(&PACKET, &saddr).await {
                        Ok(_) => {}
                        Err(err) => {
                            println_lined!("ERR: {}", err);
                            alive.store(false, Ordering::Relaxed);
                            break;
                        }
                    }
                    //println!("心跳");
                }
            })
        };

        Ok(BridgeClient {
            baddr: udp.local_addr()?,
            udp,
            saddr,
            runtimes: [pack_sender],
            name,
            alive,
        })
    }

    #[must_use]
    pub async fn recv_from(&self, buf: &mut [u8]) -> IResult<(usize, SocketAddr)> {
        if !self.is_alive() {
            return Err(anyhow::anyhow!("The bridge has broken"));
        }

        loop {
            let (len, raddr) = tokio::select! {
                result=self.udp.recv_from(buf)=>{
                    match result {
                        Ok(n) => n,
                        Err(err) => {
                            println!("`{}` read error: {}. Ignored.", self.name, err);
                            continue;
                        }
                    }
                },

                err=async{
                    loop{
                        tokio::time::sleep(Duration::from_secs(1)).await;
                        if !self.is_alive(){
                            return Err(anyhow::anyhow!("The bridge has broken"));
                        }
                    }
                }=>{
                    return err;
                }
            };

            if raddr == self.saddr {
                match buf[0] {
                    CommunicatePacket::DATA => {
                        //println!("RECV {:?}",&buf[1..len]);
                        println!("RECV");
                        buf.copy_within(1.., 0);
                        break Ok((len - 1, raddr));
                    }

                    CommunicatePacket::CLOSE => {
                        println!("CLOSE");
                        self.alive.store(false, Ordering::Relaxed);
                        break Err(anyhow::anyhow!("The bridge has broken"));
                    }

                    _ => continue,
                }
            } else {
                break Ok((len, raddr));
            }
        }
    }

    pub async fn send_to(&self, b: &[u8], d: &SocketAddr) -> IResult<()> {
        if !self.is_alive() {
            return Err(anyhow::anyhow!("The bridge has broken"));
        }

        if *d == self.saddr {
            let mut buf = [CommunicatePacket::DATA; 1500];
            (&mut buf[1..]).write(b).unwrap();
            self.udp.send_to(&buf[..b.len() + 1], &d).await?;
        } else {
            self.udp.send_to(b, d).await?;
        }

        println!("SEND");
        Ok(())
    }

    #[inline]
    pub fn is_alive(&self) -> bool {
        self.alive.load(Ordering::Relaxed)
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
        for x in self.runtimes.iter() {
            x.abort();
        }
    }
}

pub async fn log_i_result<F>(name: &str, future: F)
where
    F: std::future::Future<Output = IResult<()>>,
{
    let result = future.await;
    if let Err(ref err) = result {
        println!("`{}`已断开。因为：{}", name, err);
    }
}
