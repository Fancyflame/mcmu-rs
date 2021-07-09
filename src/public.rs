use bincode;
use serde::{Deserialize, Serialize};
use std::{
    //collections::HashMap,
    net::SocketAddr,
    sync::{Arc, Weak},
    time::Duration,
};
use tokio::{net::UdpSocket, sync::oneshot};

pub type Identity = u32;
pub type Identity2 = Identity;
pub type IResult<T> = Result<T, anyhow::Error>;

#[macro_export]
macro_rules! println_lined{
    ($($tt:tt)*)=>{
        println!("[{}:{}] {}",file!(),line!(),format!($($tt)*));
    }
}

pub enum ProxyProto<'a> {
    Connect(Identity2),
    Connected,
    Info(&'a [u8]),
    Disconnect,
    Disconnected,
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

pub struct MCPEInfo<'a> {
    splits: Vec<&'a [u8]>,
    pub world_name: String,
    pub game_port: u16,
}

pub struct BridgeClient {
    udp: Weak<UdpSocket>,
    waiting: Option<oneshot::Sender<()>>,
    saddr: SocketAddr,
    baddr: SocketAddr,
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
        pack_sender: JoinHandle<()>,
    ) -> IResult<Self> {
        let udp = Arc::new(UdpSocket::bind(baddr).await?);
        let (wake_sender, wake_receiver) = oneshot::channel::<()>();

        {
            let packet = Operate::BridgeTo(cid).serialize();
            let udp = udp.clone();

            let jh = tokio::spawn(async move {
                match {
                    tokio::select! {
                        //持续发包
                        result=async {
                            loop {
                                udp.send_to(&packet,&saddr).await?;
                                println_lined!("已发包");
                                tokio::time::sleep(Duration::from_secs(1));
                            }
                        }=>{ unreachable!() },

                        //连接成功
                        _=async {
                            wake_receiver.await.unwrap();
                        }=>{ Ok(()) }
                    }
                } {
                    Ok(_) => {
                        println!("`{}` connected", name);
                    }

                    Err(err) => {
                        println_lined!("`{}` connect failed: {}", name, err);
                    }
                }
            });
        }

        return Ok(BridgeClient {
            udp: Arc::downgrade(&udp),
            waiting: Some(wake_sender),
            baddr: udp.local_addr().unwrap(),
            saddr,
            pack_sender: jh,
        });
    }

    pub fn connected(&mut self) -> IResult<()> {
        match self.waiting {
            Some(tx) => {
                tx.send(());
                self.waiting = None;
            }

            None => {
                panic!("Connection has been established, cannot confirm the connection twice");
            }
        }

        //获取UDP套接字
        let udp = match self.udp.clone().upgrade() {
            Some(a) => a,
            None => {
                return Err(anyhow::anyhow!(
                    "Cannot establish the connection because the udp socket has dropped"
                ));
            }
        };
        let saddr = self.saddr.clone();

        //垃圾清理运行时
        tokio::spawn(async move {
            let mut static_time = 0u8;
            loop {
                //心跳检查 break后udp被析构
                tokio::time::sleep(Duration::from_secs(4)).await;
                static_time += 4;

                //当10s内处于静止状态后，开始主动发包测试
                if static_time > 10 {
                    if let Err(_) = udp.send_to(&[], saddr).await {
                        break;
                    }
                }

                //大于20s，销毁
                if static_time > 20 {
                    println!("回收");
                    break;
                }
            }
        });

        Ok(())
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

    #[inline]
    pub fn is_connected(&self) -> bool {
        self.waiting.is_none()
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
        self.pack_sender.abort();
    }
}
