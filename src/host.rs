use crate::println_lined;
use crate::public::{BridgeClient, IResult, Identity2, MCPEInfo, Operate};
use std::{collections::HashMap, net::SocketAddr, sync::Arc, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
    sync::{oneshot, Mutex},
    time,
};

pub struct Host;

impl Host {
    pub async fn run(saddr: SocketAddr) -> IResult<()> {
        let mut stream = TcpStream::connect(&saddr).await?;
        stream.write(&Operate::Open.serialize()).await?;
        let mut buf = [0u8; 64];
        let len = stream.read(&mut buf).await?;

        /*match Operate::deserialize(&buf[..len]) {
            Some(Operate::Opened(id)) => {
                println!("房间已成功开启，房间号: {}", hex::encode(&id.to_be_bytes()));
                let mut interval = tokio::time::interval(Duration::from_secs(10));

                loop {

                    tokio::select! {

                        result=stream.read(&mut buf)=>{
                            let len=result?;
                            match Operate::deserialize(&buf[..len]){

                                Some(Operate::ConnectToMe(cid1,cid2))=>{

                                    //初始化管道连接
                                    tokio::spawn(async move{
                                        let (tx,rx)=oneshot::channel::<u16>();
                                        let saddr_cloned=saddr.clone();

                                        //信息管道先运行
                                        tokio::spawn(async move{
                                            let saddr=saddr_cloned;
                                            let mut buf=[0u8;1024];
                                            let mut tx=Some(tx);
                                            let b1=BridgeClient::connect(
                                                cid1,
                                                saddr_cloned,
                                                "0.0.0.0:0".parse().unwrap(),
                                                "external info pipe",
                                            ).await?;

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

                                Some(_)=>{
                                    println_lined!("Received an unexpected packet from the server. Ignored.");
                                }

                                _other=>{
                                    println_lined!("Cannot receive correct packet from the server. Abort.");
                                    break;
                                }
                            }
                        },

                        _=interval.tick()=>{
                            stream.write(&Operate::HeartBeat.serialize()).await?;
                        }
                    }
                }
            }

            Some(Operate::OperationFailed) => {
                return Err(anyhow::anyhow!(
                    "Cannot open a room temporarily, maybe the server is full"
                ));
            }

            _ => {
                println_lined!("Malformed packet");
                dbg!(Operate::deserialize(&buf[..len]));
                return Err(anyhow::anyhow!("Received an unexpected packet from the server, maybe the version of your client is not the same to the server"));
            }
        }*/

        match Operate::deserialize(&buf[..len]){


            Some(Operate::OperationFailed)=>Err(anyhow::anyhow!("The server rejected our request")),


            Some(Operate::Opened(room_id))=>{

                println!("连接成功。房间号{}",hex::encode(room_id.to_le_bytes()));
                let waiter=Arc::new(Mutex::new(HashMap::<Identity2,oneshot::Sender<()>>::with_capacity(6)));
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

                                    let waiter=waiter.clone();
                                    tokio::spawn(async move{

                                        let info_pipe=BridgeClient::connect(
                                            cid1,
                                            saddr.clone(),
                                            SocketAddr::new([0,0,0,0].into(), 0),
                                            "info"
                                        ).await?;
                                        let (tx1,rx1)=oneshot::channel();
                                        waiter.lock().await.insert(cid1, tx1);
                                        tokio::select!{
                                            tokio::time::sleep(Duration::from_secs(secs: u64))
                                        }

                                        IResult::Ok(())

                                    });

                                }

                                //已连接的管道
                                Some(Operate::Bridged(cid))=>{
                                    match waiter.lock().await.remove(&cid){
                                        Some(o)=>{
                                            o.send(());
                                        }
                                        None=>{
                                            println_lined!("A `Bridged` packet sent from the server doesn't match any connections. Ignored.");
                                        }
                                    }
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
