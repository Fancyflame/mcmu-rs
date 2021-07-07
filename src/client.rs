use crate::println_lined;
use crate::public::{BridgeClient, IResult, Identity, MCPEInfo, Operate};
use std::net::{IpAddr, SocketAddr};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpStream, UdpSocket},
};

pub struct Client;

impl Client {
    pub async fn run(saddr: SocketAddr, cid: Identity) -> IResult<()> {
        macro_rules! catch {
            ($expr:expr) => {
                match $expr {
                    Ok(ok) => ok,
                    Err(err) => {
                        println_lined!("Error: {}", err);
                        return;
                    }
                }
            };
        }

        let mut tcp = TcpStream::connect(saddr.clone()).await?;
        let mut tcp_buf = [0u8; 64];
        tcp.write(&Operate::HelloTo(cid).serialize()).await?;
        let len = tcp.read(&mut tcp_buf).await?;
        match Operate::deserialize(&tcp_buf[..len]) {
            Some(Operate::ConnectToMe(cid1, cid2)) => {
                println!("Connected to the server successfully! Connecting to the room ...");
                //记录游戏管道端口，以便后期魔改
                let (tx, rx) = tokio::sync::oneshot::channel::<u16>();
                lazy_static! {
                    static ref LOCALHOST: IpAddr = [127, 0, 0, 1].into();
                }

                //游戏管道
                let game_pipe = tokio::spawn(async move {
                    let mut buf = [0u8; 2048];
                    let udp = UdpSocket::bind("0.0.0.0:0".parse::<SocketAddr>().unwrap())
                        .await
                        .unwrap();
                    tx.send(udp.local_addr().unwrap().port()).unwrap();
                    let b2 = catch!(
                        BridgeClient::connect(cid2, saddr, udp, Some("client game pipe")).await
                    );

                    let mut mc_addr: Option<SocketAddr> = None;
                    loop {
                        let (len, raddr) = match b2.recv_from(&mut buf).await {
                            Some(v) => v,
                            None => {
                                println!("`game` pipe has disconnected");
                                break;
                            }
                        };

                        let saddr = b2.saddr();
                        match mc_addr {
                            None => {
                                if raddr.ip() == *LOCALHOST {
                                    mc_addr = Some(raddr);
                                    b2.send_to(&buf[..len], saddr).await;
                                }
                            }
                            Some(ref laddr) => {
                                if raddr == *laddr {
                                    b2.send_to(&buf[..len], saddr).await;
                                } else if raddr == *saddr {
                                    b2.send_to(&buf[..len], laddr).await;
                                }
                            }
                        }
                    }
                });

                let gport = match rx.await {
                    Ok(v) => v,
                    Err(err) => {
                        return Err(std::io::Error::new(std::io::ErrorKind::BrokenPipe, err))
                    }
                };

                //信息管道
                let saddr_cloned = saddr.clone();
                let info_pipe = tokio::spawn(async move {
                    let mut buf = [0u8; 1024];
                    let b1 = catch!(
                        BridgeClient::bind(
                            cid1,
                            saddr_cloned,
                            "127.0.0.1:19132".parse().unwrap(),
                            Some("client info pipe")
                        )
                        .await
                    );

                    let mut mc_addr: Option<SocketAddr> = None;
                    let mut cache_check = Vec::new();
                    let mut cache_load = Vec::new();
                    loop {
                        let (len, raddr) = match b1.recv_from(&mut buf).await {
                            Some(v) => v,
                            None => {
                                println!("`info` pipe has disconnected");
                                break;
                            }
                        };

                        let saddr = b1.saddr();
                        match mc_addr {
                            None => {
                                if raddr.ip() == *LOCALHOST {
                                    mc_addr = Some(raddr);
                                    b1.send_to(&buf[..len], saddr).await;
                                }
                            }
                            Some(ref laddr) => {
                                let bytes = &buf[..len];
                                if raddr == *laddr {
                                    b1.send_to(bytes, saddr).await;
                                } else if raddr == *saddr {
                                    if bytes != &cache_check {
                                        println!("Unconnected ping modified");
                                        let mut info = match MCPEInfo::deserialize(bytes) {
                                            Some(v) => v,
                                            None => {
                                                println_lined!("The protocol is malformed");
                                                continue;
                                            }
                                        };
                                        info.game_port = gport;
                                        cache_check = bytes.to_owned();
                                        cache_load = info.serialize();
                                    }
                                    b1.send_to(&cache_load, laddr).await;
                                }
                            }
                        }
                    }
                });

                tokio::try_join!(info_pipe, game_pipe).unwrap();
            }
            Some(Operate::OperationFailed) => {
                println!("连接失败。请核对您的房间号是否正确");
            }
            _ => {
                println!("Oops! There might be something wrong with the server");
            }
        }
        Ok(())
    }
}
