use crate::println_lined;
use crate::public::*;
use std::{
    net::SocketAddr,
    sync::{atomic::AtomicU16, Arc},
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
    sync::{oneshot, Mutex},
    task::JoinHandle,
    time,
};

pub struct Host;

impl Host {
    pub async fn run(saddr: SocketAddr) -> IResult<()> {
        let mut stream = TcpStream::connect(&saddr).await?;
        stream.write(&Operate::Open.serialize()).await?;
        let mut buf = [0u8; 64];
        let len = stream.read(&mut buf).await?;

        match Operate::deserialize(&buf[..len]){


            Some(Operate::OperationFailed)=>Err(anyhow::anyhow!("The server rejected our request")),


            Some(Operate::Opened(room_id))=>{

                println!("连接成功。房间号{:07}",room_id);
                let mut interval=time::interval(Duration::from_secs(10));
                loop{

                    tokio::select!{

                        //读取到数据包
                        result=stream.read(&mut buf)=>{

                            //流结束
                            if let Ok(0)=result{
                                break;
                            }

                            let len=result?;
                            match Operate::deserialize(&buf[..len]){

                                //有连接
                                Some(Operate::ConnectToMe(cid1,cid2))=>{

                                    tokio::spawn(async move{
                                        let (tx,rx)=oneshot::channel::<u16>();
                                        let mut tx_option=Some(tx);

                                        //信息管道
                                        let _info=tokio::spawn(log_i_result("房主信息管道", async move{

                                            let info_pipe=BridgeClient::connect(cid1, saddr.clone(), SocketAddr::new([0,0,0,0].into(), 0), "房主信息管道").await?;
                                            let laddr=SocketAddr::new([127,0,0,1].into(), 19132);
                                            let mut buf=[0u8;1500];
                                            buf[0]=CommunicatePacket::DATA;

                                            loop{
                                                let (len,raddr)=match info_pipe.recv_from(&mut buf[1..]).await{
                                                    Ok(s)=>s,
                                                    Err(_)=>{
                                                        tokio::time::sleep(Duration::from_millis(100)).await;
                                                        continue;
                                                    }
                                                };

                                                if raddr==laddr{
                                                    //从本地发来
                                                    if tx_option.is_some(){
                                                        //还没有发送过端口号
                                                        let info = match MCPEInfo::deserialize(&buf[1..len+1]) {
                                                            Some(v) => v,
                                                            None => {
                                                                println_lined!("The protocol is malformed");
                                                                continue;
                                                            }
                                                        };
                                                        std::mem::replace(&mut tx_option, None)
                                                            .unwrap()
                                                            .send(info.game_port).unwrap();
                                                    }
                                                    info_pipe.send_to(&buf[..len+1],&saddr).await?;
                                                }else if raddr==saddr{
                                                    //从远程玩家发来
                                                    info_pipe.send_to(&buf[2..len+1],&laddr).await?;
                                                }else{
                                                    println_lined!("Received a packet from unknown remote address: {}. Ignored.",raddr);
                                                }
                                            }

                                        }));

                                        //游戏管道
                                        let game=tokio::spawn(log_i_result("房主游戏管道",async move{

                                            let game_pipe=BridgeClient::connect(cid2, saddr.clone(), SocketAddr::new([0,0,0,0].into(), 0), "房主游戏管道").await?;
                                            let laddr=SocketAddr::new([127,0,0,1].into(), rx.await?);
                                            let mut buf=[0u8;1500];
                                            buf[0]=CommunicatePacket::DATA;

                                            loop{
                                                let (len,raddr)=game_pipe.recv_from(&mut buf[1..]).await?;
                                                if raddr==laddr{
                                                    //从本地发来
                                                    game_pipe.send_to(&buf[..len+1],&saddr).await?;
                                                }else if raddr==saddr{
                                                    //从远程玩家发来
                                                    game_pipe.send_to(&buf[2..len+1],&laddr).await?;
                                                }
                                            }
                                        }));

                                        game.await;
                                        //dbg!(tokio::try_join!(game));
                                        println!("游戏管道断开。玩家退出房间。");
                                    });


                                }

                                _=>{
                                    println_lined!("Received an Unexpected packet from the server. Ignored.");
                                    stream.write(&Operate::OperationFailed.serialize()).await?;
                                }

                            }
                        },

                        //心跳包
                        _=interval.tick()=>{
                            stream.write(&Operate::HeartBeat.serialize()).await?;
                        }

                    }

                }
                Ok(())

            },


            _=>Err(anyhow::anyhow!("The server returned an invalid packet. Maybe either your or server's version is outdated.")),


        }
    }
}
