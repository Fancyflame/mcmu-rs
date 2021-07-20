use bincode;
use serde::{Deserialize, Serialize};
use std::{
    io::Write,
    net::SocketAddr,
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
    udp: Arc<UdpSocket>,
    saddr: SocketAddr,
    baddr: SocketAddr,
    runtimes: [JoinHandle<IResult<()>>; 2],
    alive: AtomicBool,
    //buffer:ring_buffer::Reader
}

impl CommunicatePacket {
    pub const CONNECT: u8 = 0;
    //pub const ACK: u8 = 1;
    pub const DATA: u8 = 2;
    pub const HEARTBEAT: u8 = 3;
    //pub const RESET: u8 = 4;
    pub const CLOSE: u8 = 5;
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
    pub async fn connect<F>(
        cid: Identity2,
        saddr: SocketAddr, //服务器的地址
        baddr: SocketAddr, //绑定的地址
        name: &'static str,
        handler: F,
    ) -> IResult<Self>
    where
        F: FnMut(&[u8],SocketAddr) -> IResult<()>,
    {
        let udp = Arc::new(UdpSocket::bind(baddr).await?);
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
                    tokio::time::sleep(Duration::from_millis(500)).await;
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
        let pack_sender: JoinHandle<IResult<()>> = {
            let udp = udp.clone();
            let saddr = saddr.clone();
            tokio::spawn(async move {
                let packet = [CommunicatePacket::HEARTBEAT];
                let mut interval = tokio::time::interval(Duration::from_secs(2));
                loop {
                    interval.tick().await;
                    udp.send_to(&packet, &saddr).await?;
                }
            })
        };


        //TODO 改为函数式，增添&mut [u8]数组储存
        //处理数据
        let pack_receiver: JoinHandle<IResult<()>> = {
            let udp = udp.clone();
            let mut buf=[0u8;1500];
            tokio::spawn(async move {
                loop {
                    let (len,raddr)=match udp.recv_from(&mut buf).await{
                        Ok(n)=>n,
                        Err(err)=>{
                            println!("`{}` read error: {}. Ignored.",);
                            continue;
                        }
                    };

                    if raddr == self.saddr {
                        match b[0] {
                            CommunicatePacket::DATA => {
                                println!("RECV {:?}", &b[1..bundle.0]);
                                handler(&buf[1..len])?;
                            }
                            CommunicatePacket::CLOSE => {
                                self.alive.store(false, Ordering::Relaxed);
                                break Err(anyhow::anyhow!("The Arc has been dropped"));
                            }
                            _ => {},
                        }
                    } else {
                        handler(&buf[1..len])?;
                    }
                }

            });
        };

        Ok(BridgeClient {
            baddr: udp.local_addr()?,
            udp,
            saddr,
            runtimes: [pack_sender, pack_receiver],
            alive: AtomicBool::new(true),
        })
    }

    pub async fn recv_from(&self, b: &mut [u8]) -> IResult<(usize, SocketAddr)> {
        unimplemented!();
    }

    pub async fn send_to(&self, b: &[u8], d: &SocketAddr) -> IResult<()> {
        if !self.alive() {
            return Err(anyhow::anyhow!("The Arc has been dropped"));
        }

        self.udp.send_to(b, d).await?;
        println!("SEND");
        Ok(())
    }

    pub fn alive(&self) -> bool {
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
        for x in self.runtimes.iter_mut() {
            x.abort();
        }
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
