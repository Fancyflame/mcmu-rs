use crate::println_lined;
use crate::public::{BridgeClient, IResult, MCPEInfo, Operate};
use std::{net::SocketAddr, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
    sync::oneshot,
};

pub struct Host;

impl Host {
    pub async fn run(saddr: SocketAddr)->IResult<()>{


        let mut stream = TcpStream::connect(&saddr).await?;
        stream.write(&Operate::Open.serialize()).await?;
        let mut buf = [0u8; 512];
        let len = stream.read(&mut buf).await?;


        match Operate::deserialize(&buf[..len]) {


            Some(Operate::Opened(id)) => {
                println!("房间已成功开启，房间号: {}", hex::encode(&id.to_be_bytes()));
                let mut interval = tokio::time::interval(Duration::from_secs(10));

                loop {
                    tokio::select! {
                        result=stream.read(&mut buf)=>match result{
                            Ok(len)=>{
                                match Operate::deserialize(&buf[..len]){
                                    Some(Operate::ConnectToMe(cid1,cid2))=>{
                                        //初始化管道连接

                                        tokio::spawn(async move{
                                            macro_rules! catch{
                                                ($expr:expr)=>{
                                                    match $expr{
                                                        Ok(ok)=>ok,
                                                        Err(err)=>{
                                                            println_lined!("Error occurred in player pipe, abort: {}",err);
                                                            return;
                                                        }
                                                    }
                                                }
                                            }

                                            let (tx,rx)=oneshot::channel::<u16>();

                                            let saddr_cloned=saddr.clone();
                                            //信息管道先运行
                                            tokio::spawn(async move{
                                                let saddr=saddr_cloned;
                                                let mut buf=[0u8;1024];
                                                let mut tx=Some(tx);
                                                let b1=catch!(BridgeClient::bind(
                                                    cid1,
                                                    saddr_cloned,
                                                    "0.0.0.0:0".parse().unwrap(),
                                                    Some("external info pipe"),
                                                ).await);

                                                lazy_static!{
                                                    static ref LADDR:SocketAddr=SocketAddr::new([127,0,0,1].into(),19132);
                                                }


                                                loop{
                                                    let (len,raddr)=match b1.recv_from(&mut buf).await{
                                                        Some(v)=>v,
                                                        None=>{
                                                            println!("`info` pipe has disconnected");
                                                            break;
                                                        }
                                                    };

                                                    if raddr==saddr {
                                                        b1.send_to(&buf[..len],&LADDR).await.unwrap_or(());
                                                        if let Some(tx_)=std::mem::replace(&mut tx,None){
                                                            match MCPEInfo::deserialize(&buf[..len]){
                                                                Some(info)=>{
                                                                    catch!(tx_.send(info.game_port));
                                                                    tx=None;
                                                                },
                                                                None=>{
                                                                    println_lined!("Weird parsing error, maybe minecraft protocol has changed.");
                                                                    tx=Some(tx_);
                                                                }
                                                            };
                                                        }
                                                    }else if raddr==*LADDR{
                                                        b1.send_to(&buf[..len],&saddr).await.unwrap_or(());
                                                    }else{
                                                        println_lined!("Invalid address: {}",raddr);
                                                    }
                                                }
                                            });

                                            //游戏管道后运行
                                            let game_port=catch!(rx.await);
                                            {
                                                //本着节约的原则，就不新开任务了
                                                let mut buf=[0u8;2048];
                                                let laddr=SocketAddr::new([127,0,0,1].into(),game_port);
                                                let b2=catch!(BridgeClient::bind(
                                                    cid2,
                                                    saddr,
                                                    "0.0.0.0:0".parse().unwrap(),
                                                    Some("external game pipe")
                                                ).await);

                                                loop{
                                                    let (len,raddr)=match b2.recv_from(&mut buf).await{
                                                        Some(v)=>v,
                                                        None=>{
                                                            println!("`game` pipe has disconnected");
                                                            break;
                                                        }
                                                    };
                                                    if raddr==saddr{
                                                        b2.send_to(&buf[..len],&laddr).await;
                                                    }else if raddr==laddr{
                                                        b2.send_to(&buf[..len],&saddr).await;
                                                    }else{
                                                        println_lined!("Invalid address: {}",raddr);
                                                    }
                                                }
                                            }


                                        });

                                    },

                                    _other=>{
                                        println_lined!("Cannot receive correct packet from the server. Abort.");
                                        break;
                                    }
                                }
                            },

                            Err(_)=>{
                                println!("Connection to the server has broke, players cannot join your room\
                                    now, but players have joined can continue the game");
                                break;
                            }
                        },

                        _=interval.tick()=>{
                            stream.write(&Operate::HeartBeat.serialize()).await?;
                        }
                    }
                }
            }


            Some(Operate::OperationFailed) => {
                println!("Cannot open a room temporarily, maybe the server is full");
            }


            _ => {
                println_lined!("There is something wrong with the server");
                dbg!(Operate::deserialize(&buf[..len]));
            }


        }

        Ok(())
    }
}
