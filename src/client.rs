use crate::println_lined;
use crate::public::{log_i_result, BridgeClient, IResult, Identity, MCPEInfo, Operate};
use std::net::{IpAddr, SocketAddr};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
    task::JoinHandle,
};

pub struct Client;

impl Client {
    pub async fn run(saddr: SocketAddr, cid: Identity) -> IResult<()> {
        let mut tcp = TcpStream::connect(saddr.clone()).await?;
        let mut tcp_buf = [0u8; 32];
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
                let game_pipe: JoinHandle<IResult<()>> = {
                    let saddr = saddr.clone();
                    tokio::spawn(log_i_result("游戏管道", async move {
                        let mut buf = [0u8; 1024];
                        let b2 = BridgeClient::connect(
                            cid2,
                            saddr,
                            SocketAddr::new([0, 0, 0, 0].into(), 0),
                            "客户端游戏管道",
                        )
                        .await?;

                        if let Err(_) = tx.send(b2.baddr().port()) {
                            return Err(anyhow::anyhow!("Cannot send game port to info pipe!"));
                        }

                        let mut mc_addr: Option<SocketAddr> = None;
                        loop {
                            let (len, raddr) = match b2.recv_from(&mut buf).await {
                                Ok(v) => v,
                                Err(err) => {
                                    println!("`game` pipe has disconnected: {}", err);
                                    break;
                                }
                            };

                            let saddr = b2.saddr();
                            match mc_addr {
                                None => {
                                    if raddr.ip() != saddr.ip() {
                                        mc_addr = Some(raddr);
                                        b2.send_to(&buf[..len], saddr).await?;
                                    }
                                }
                                Some(ref laddr) => {
                                    if raddr == *laddr {
                                        b2.send_to(&buf[..len], saddr).await?;
                                    } else if raddr == *saddr {
                                        b2.send_to(&buf[..len], laddr).await?;
                                    }
                                }
                            }
                        }
                        Ok(())
                    }))
                };

                //信息管道
                let info_pipe = {
                    let saddr = saddr.clone();
                    tokio::spawn(log_i_result("信息管道", async move {
                        let mut buf = [0u8; 1024];
                        let b1 = BridgeClient::connect(
                            cid1,
                            saddr.clone(),
                            "127.0.0.1:19132".parse().unwrap(),
                            "客户端信息管道",
                        )
                        .await?;

                        let gport = rx.await?;
                        let mut laddr_option: Option<SocketAddr> = None;
                        let mut cache_check = Vec::new();
                        let mut cache_load = Vec::new();
                        loop {
                            let (len, raddr) = match b1.recv_from(&mut buf).await {
                                Ok(v) => v,
                                Err(err) => {
                                    println!("`info` pipe has disconnected: {}", err);
                                    break;
                                }
                            };

                            let saddr = b1.saddr();
                            match laddr_option {
                                None => {
                                    if raddr.ip() != saddr.ip() {
                                        laddr_option = Some(raddr);
                                        b1.send_to(&buf[..len], saddr).await?;
                                    }
                                }

                                Some(ref laddr) => {
                                    let bytes = &buf[..len];
                                    if raddr == *laddr {
                                        //从本地主机发来
                                        b1.send_to(bytes, saddr).await?;
                                    } else if raddr == *saddr {
                                        //从服务器发来（需要修改）
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
                                        b1.send_to(&cache_load, laddr).await?;
                                    }
                                }
                            }
                        }
                        Ok(())
                    }))
                };

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
