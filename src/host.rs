use crate::println_lined;
use crate::public::{log_i_result, BridgeClient, IResult, MCPEInfo, Operate};
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
                                    let (tx,rx)=oneshot::channel::<u16>();
                                    let mut tx_option=Some(tx);

                                    //信息管道
                                    let info=tokio::spawn(log_i_result("房主信息管道", async move{

                                        let info_pipe=BridgeClient::connect(cid1, saddr.clone(), SocketAddr::new([0,0,0,0].into(), 0), "房主信息管道").await?;
                                        let laddr=SocketAddr::new([127,0,0,1].into(), 19132);
                                        let mut buf=[0u8;1024];
                                        loop{
                                            let (len,raddr)=info_pipe.recv_from(&mut buf).await?;

                                            if raddr==laddr{
                                                //从本地发来
                                                if tx_option.is_some(){
                                                    //还没有发送过端口号
                                                    let info = match MCPEInfo::deserialize(&buf[..len]) {
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
                                                drop(info_pipe.send_to(&buf[..len],&saddr).await);
                                            }else if raddr==saddr{
                                                //从玩家发来
                                                info_pipe.send_to(&buf[..len],&laddr).await?;
                                            }else{
                                                println_lined!("Received a packet from unknown remote address: {}. Ignored.",raddr);
                                            }
                                        }

                                    }));

                                    //游戏管道
                                    let game=tokio::spawn(log_i_result("房主游戏管道",async move{
                                        let game_pipe=BridgeClient::connect(cid2, saddr.clone(), SocketAddr::new([0,0,0,0].into(), 0), "房主游戏管道").await?;
                                        let laddr=SocketAddr::new([127,0,0,1].into(), rx.await?);
                                        let mut buf=[0u8;1024];
                                        loop{
                                            let (len,raddr)=game_pipe.recv_from(&mut buf).await?;
                                            if raddr==laddr{
                                                //从本地发来
                                                game_pipe.send_to(&buf[..len],&saddr).await?;
                                            }else if raddr==saddr{
                                                //从玩家发来
                                                game_pipe.send_to(&buf[..len],&laddr).await?;
                                            }
                                        }
                                    }));

                                    dbg!(tokio::try_join!(info,game));

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
