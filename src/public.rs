use bincode;
use serde::{Deserialize, Serialize};
use std::{
    convert::TryInto,
    //collections::HashMap,
    net::SocketAddr,
    sync::{Arc, Weak},
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
    Bridged(Identity2),
    OperationFailed,
    HeartBeat,
}

pub enum CommunicatePacket {
    Connect = 0,
    Data = 1,
    HeartBeat = 2,
}

pub struct MCPEInfo<'a> {
    splits: Vec<&'a [u8]>,
    pub world_name: String,
    pub game_port: u16,
}

pub struct BridgeClient {
    udp: Weak<UdpSocket>,
    saddr: SocketAddr,
    baddr: SocketAddr,
    gc: JoinHandle<()>,
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
        slices[4] = game_port1.as_bytes();
        slices[5] = game_port2.as_bytes();

        return slices.concat();
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

        let packet = {
            let mut packet = [0u8; 5];
            packet[0] = CommunicatePacket::Connect as u8;
            std::mem::replace(&mut packet[1..5].try_into().unwrap(), cid.to_be_bytes());
            packet
        };

        //间隔发包
        let pinger = {
            let udp = udp.clone();
            tokio::spawn(async move {
                loop {
                    if let Err(err) = udp.send_to(&packet, &saddr).await {
                        println_lined!("Unexpected error:{}", err);
                        continue;
                    }
                    println_lined!("已发包");
                    tokio::time::sleep(Duration::from_millis(500));
                }
            })
        };

        //监听是否连接成功
        {
            let udp = udp.clone();
            let saddr = saddr.clone();
            let result = tokio::time::timeout(Duration::from_secs(10), async move {
                let mut buf = [0u8; 5];
                loop {
                    let (len, raddr) = udp.recv_from(&mut buf).await?;
                    if raddr == saddr && buf[..len] == packet {
                        break IResult::Ok(());
                    }
                }
            })
            .await;
            pinger.abort();
            result??;
        }

        //垃圾清理运行时
        let gc = {
            let udp = udp.clone();
            tokio::spawn(async move {
                let mut static_time = 0u8;
                let mut interval = tokio::time::interval(Duration::from_secs(4));
                loop {
                    //心跳检查 break后udp被析构
                    interval.tick().await;
                    static_time += 4;

                    //当10s内处于静止状态后，开始主动发包测试
                    if static_time > 10 {
                        if let Err(_) = udp
                            .send_to(&[CommunicatePacket::HeartBeat as u8], saddr)
                            .await
                        {
                            break;
                        }
                    }

                    //大于20s，销毁
                    if static_time > 20 {
                        println!("回收");
                        break;
                    }
                }
            })
        };

        Ok(BridgeClient {
            udp: Arc::downgrade(&udp),
            saddr,
            baddr: udp.local_addr()?,
            gc,
        })
    }

    pub async fn recv_from(&self, b: &mut [u8]) -> Option<(usize, SocketAddr)> {
        match self.udp.upgrade() {
            Some(arc) => arc.recv_from(b).await.ok(),
            None => None,
        }
    }

    pub async fn send_to(&self, b: &[u8], d: &SocketAddr) -> Option<()> {
        match self.udp.upgrade() {
            Some(arc) => {
                arc.send_to(b, d).await.ok()?;
                Some(())
            }
            None => None,
        }
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
        self.gc.abort();
    }
}
