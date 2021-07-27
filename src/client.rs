use crate::println_lined;
use crate::public::*;
use std::{
    net::{IpAddr, SocketAddr},
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
    task::JoinHandle,
};

pub struct Client;

impl Client {
    pub async fn run(saddr: SocketAddr, cid: Identity) -> IResult<()> {
        println!(
            /*"局域网玩家连接法：因为Minecraft本身限制，需要于Minecraft服务器列表中添加\
            一个服务器，服务器名任意，服务器地址为127.0.0.1，端口为19138，\
            仅添加，不需要点击进入。适用于非Xbox的玩家。\n\n\*/ "局域网玩家连接法：启动mcmu，在好友列表会自动出现房间。\n\n\
            服务器列表连接法：添加一个服务器，服务器名任意，服务器地址为127.0.0.1，端口\
            为40010。玩家可以从服务器列表直接点击进入房间，适用于Xbox玩家。\n\n\
            可以同时添加上述两个服务器，任意选择使用。\n\n"
        );
        let mut tcp = TcpStream::connect(saddr.clone()).await?;
        let mut tcp_buf = [0u8; 32];
        tcp.write(&Operate::HelloTo(cid).serialize()).await?;

        let len = tcp.read(&mut tcp_buf).await?;
        match Operate::deserialize(&tcp_buf[..len]) {
            Some(Operate::ConnectToMe(cid1, cid2)) => {
                drop(tcp);
                println!("成功连接到服务器，正在连接到房主...");

                //记录游戏管道端口，以便后期魔改
                let (tx, rx) = tokio::sync::oneshot::channel::<u16>();

                //游戏管道
                let game_pipe = {
                    let saddr = saddr.clone();
                    tokio::spawn(log_i_result("游戏管道", async move {
                        let b2 = BridgeClient::connect(
                            cid2,
                            saddr,
                            SocketAddr::new([0, 0, 0, 0].into(), 40010),
                            "客户端游戏管道",
                        )
                        .await?;

                        if let Err(_) = tx.send(b2.baddr().port()) {
                            return Err(anyhow::anyhow!("Cannot send game port to info pipe!"));
                        }

                        let mut mc_addr: Option<SocketAddr> = None;
                        let mut buf = [0u8; 1500];

                        loop {
                            let (len, raddr) = match b2.recv_from(&mut buf).await {
                                Ok(v) => v,
                                Err(err) => {
                                    println!("客户端游戏管道已断开，因为: {}", err);
                                    break;
                                }
                            };

                            let saddr = b2.saddr();
                            match mc_addr {
                                None => {
                                    if raddr.ip() == *LOCALADDR {
                                        mc_addr = Some(raddr);
                                        b2.send_to(&buf[..len], saddr).await?;
                                    }
                                }
                                Some(ref laddr) => {
                                    if raddr == *laddr {
                                        //从本地发来
                                        println!("本地数据");
                                        b2.send_to(&buf[..len], saddr).await?;
                                    } else if raddr == *saddr {
                                        //从房主发来
                                        println!("远程数据");
                                        b2.send_to(&buf[..len], laddr).await?;
                                    }
                                }
                            }
                        }
                        Ok(())
                    }))
                };

                tokio::time::sleep(std::time::Duration::from_millis(10)).await;

                //信息管道
                let _info_pipe = {
                    let saddr = saddr.clone();
                    tokio::spawn(log_i_result("信息管道", async move {
                        let b1 = BridgeClient::connect(
                            cid1,
                            saddr.clone(),
                            SocketAddr::new(
                                [0, 0, 0, 0].into(),
                                if cfg!(target_os = "windows") {
                                    19132
                                } else {
                                    19132 //安卓也需要添加服务器列表了
                                },
                            ),
                            "客户端信息管道",
                        )
                        .await?;

                        let gport = rx.await?;
                        let mut laddr_option: Option<SocketAddr> = None;
                        let mut cache_check = Vec::new();
                        let mut cache_load = Vec::new();
                        let mut buf = [0u8; 1500];

                        loop {
                            let (len, raddr) = match b1.recv_from(&mut buf).await {
                                Ok(v) => v,
                                Err(err) => {
                                    println!("客户端信息管道已断开，因为：{}", err);
                                    break;
                                }
                            };

                            let saddr = b1.saddr();
                            match laddr_option {
                                None => {
                                    if raddr.ip() != saddr.ip() {
                                        let mut addr = raddr;
                                        addr.set_ip(*LOCALADDR);
                                        laddr_option = Some(addr);
                                        b1.send_to(&buf[..len], saddr).await?;
                                    }
                                }

                                Some(ref laddr) => {
                                    if raddr == *laddr {
                                        //从本地主机发来
                                        b1.send_to(&buf[..len], saddr).await?;
                                    } else if raddr == *saddr {
                                        //从服务器发来（需要修改）
                                        let data = &buf[..len];
                                        if data != &cache_check {
                                            println!("Unconnected ping modified");
                                            let mut info = match MCPEInfo::deserialize(data) {
                                                Some(v) => v,
                                                None => {
                                                    println_lined!("The protocol is malformed");
                                                    continue;
                                                }
                                            };
                                            info.game_port = gport;
                                            cache_check = data.to_owned();
                                            cache_load = info.serialize();
                                        }
                                        println_lined!("{}", String::from_utf8_lossy(&cache_load));
                                        lazy_static! {
                                            static ref STREAM:Vec<u8>=hex::decode(
                                                "1c0000000000d1fe2180f5403287a99bb200ffff00fefefefefdfdfdfd12345678005d4d4350453b46616e6379466c616d65583b3435363b312e31372e32302e32323b313b383b31333738363935373235393131343130313130353be68891e79a84e4b896e7958c3b43726561746976653b313b35343339323b35343339333b"
                                            ).unwrap();
                                        }

                                        for _ in 0..3 {
                                            b1.send_to(&cache_load, laddr).await?;
                                            tokio::time::sleep(Duration::from_millis(500)).await;
                                        }
                                    }
                                }
                            }
                        }
                        Ok(())
                    }))
                };

                drop(game_pipe.await);
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
