use crate::println_lined;
use crate::public::{IResult, Identity, Identity2, Operate};
use rand::{random, Rng};
use std::{
    collections::HashMap,
    net::{IpAddr, SocketAddr},
    sync::{
        atomic::{AtomicU32, Ordering},
        Arc,
    },
    time::Duration,
};
use tokio::{
    self,
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener /*TcpStream, ToSocketAddrs*/, UdpSocket},
    sync::{mpsc, Mutex, RwLock},
    task::JoinHandle,
    time,
};

pub struct Server;

struct Room {
    tx: mpsc::Sender<(Identity2, Identity2, IpAddr)>, //pwd:
}

struct Bridge {
    waiter: Option<SocketAddr>,
    expected_ip: (IpAddr, IpAddr),
    timeout: u8, //超过一定时间后销毁
}

impl Bridge {
    fn new(ip1: IpAddr, ip2: IpAddr) -> Self {
        Bridge {
            waiter: None,
            expected_ip: (ip1, ip2),
            timeout: 0,
        }
    }
}

impl Server {
    pub async fn run(addr: SocketAddr) -> IResult<()> {
        let bridge_counter = Arc::new(AtomicU32::new(random()));
        let bridges = Arc::new(Mutex::new(HashMap::<Identity2, Bridge>::new()));
        //转发表
        let table = Arc::new(RwLock::new(HashMap::<SocketAddr, SocketAddr>::new()));

        //构建连接的通信服务器
        let connecter: JoinHandle<IResult<()>> = {
            let rooms = Arc::new(RwLock::new(HashMap::<Identity, Room>::new()));
            let bridges = Arc::clone(&bridges);
            let tcp = TcpListener::bind(&addr).await?;

            tokio::spawn(async move {
                loop {
                    let bridge_counter = Arc::clone(&bridge_counter);
                    let rooms = Arc::clone(&rooms);
                    let bridges = Arc::clone(&bridges);
                    let (mut stream, haddr) = tcp.accept().await?;

                    tokio::spawn(async move {
                        let result=async {


                            macro_rules! fail{
                                ()=>{
                                    stream.write(&Operate::OperationFailed.serialize()).await.and(Ok(()))?
                                }
                            }


                            let mut buf=[0u8;64];
                            let len=time::timeout(
                                Duration::from_secs(5),
                                stream.read(&mut buf)
                            ).await??;

                            //开始建立通信
                            match Operate::deserialize(&buf[..len]){


                                //开服
                                Some(Operate::Open)=>{


                                    let mut rooms_lock=rooms.write().await;
                                    //当一个连接成功后从这里发回信息
                                    let (ok_tx,mut ok_rx)=mpsc::channel::<Identity2>(4);

                                    //已经没位置了
                                    if rooms_lock.len()>=0xffffffff{
                                        return Err(anyhow::anyhow!("THE ROOM POOL IS FULL!"));
                                    }

                                    //寻找可用的id
                                    let room_id=loop{
                                        let id=rand::thread_rng().gen_range(0..1000000);
                                        if !rooms_lock.contains_key(&id){
                                            break id;
                                        }
                                    };

                                    //已开服
                                    let (tx,mut rx)=mpsc::channel(3);
                                    stream.write(&Operate::Opened(room_id).serialize()).await?;
                                    rooms_lock.insert(room_id,Room{ tx });
                                    //drop(rooms_lock);


                                    //Runtime
                                    loop{


                                        tokio::select!{


                                            //有玩家连接
                                            opt=rx.recv()=>{

                                                let (cid1,cid2,rip)=opt.unwrap();
                                                let mut bridges_lock=bridges.lock().await;
                                                bridges_lock.insert(cid1,Bridge::new(haddr.ip(),rip));
                                                bridges_lock.insert(cid2,Bridge::new(haddr.ip(),rip));
                                                stream.write(&Operate::ConnectToMe(cid1,cid2).serialize()).await?;

                                            },

                                            //已完成一个连接
                                            id2=ok_rx.recv()=>{
                                                stream.write(&Operate::Bridged(id2.unwrap()).serialize()).await?;
                                            },

                                            //房主有动作
                                            result=stream.read(&mut buf)=>match result{

                                                //关闭房间
                                                Err(_) | Ok(0)=>{
                                                    rooms.write().await.remove(&room_id);
                                                    break;
                                                },

                                                //心跳包或者其他数据
                                                Ok(_)=>{
                                                    //目前没有什么数据是有用的，包括心跳包
                                                    //println!("recv!")
                                                }

                                            }


                                        };

                                    }
                                },


                                //连接到房间
                                Some(Operate::HelloTo(id))=>{

                                    let rooms_lock=rooms.read().await;
                                    match rooms_lock.get(&id){

                                        Some(room)=>{

                                            let bridges_lock=bridges.lock().await;
                                            let find_id=||{
                                                let mut val=bridge_counter.load(Ordering::Relaxed);
                                                loop{
                                                    if val==0xffffffff{
                                                        val=0;
                                                    }else{
                                                        val+=1;
                                                    }

                                                    if !bridges_lock.contains_key(&val){
                                                        bridge_counter.store(val,Ordering::Relaxed);
                                                        break val;
                                                    }
                                                }
                                            };

                                            let cid1=find_id();
                                            let cid2=find_id();
                                            room.tx.send((cid1,cid2,haddr.ip())).await.unwrap(); //把连接信息发给房主
                                            stream.write(&Operate::ConnectToMe(cid1,cid2).serialize()).await?;

                                        },

                                        None=>fail!()

                                    }

                                }

                                //未知
                                _=>fail!()

                            }

                            Ok(())


                        }.await;

                        match result {
                            Ok(_) => {}
                            Err(err) => {
                                println_lined!("ERR! {}", err);
                            }
                        }
                    });
                }
            })
        };

        //桥接与转发
        {
            let socket = Arc::new(UdpSocket::bind(&addr).await?);
            let bridges = Arc::clone(&bridges);
            let table = Arc::clone(&table);

            tokio::spawn(async move {
                let socket = Arc::clone(&socket);
                let bridges = Arc::clone(&bridges);
                let table = Arc::clone(&table);

                tokio::spawn(async move {
                    let mut buf = [0u8; 1472]; //UDP最大安全大小
                    loop {
                        match socket.recv_from(&mut buf).await {
                            Ok((len, addr)) => match table.read().await.get(&addr) {
                                Some(raddr) => {
                                    if len == 0 {
                                        continue;
                                    }
                                    socket.send_to(&buf[..len], raddr).await.unwrap();
                                }

                                //桥接
                                None => {
                                    match Operate::deserialize(&buf[..len]) {
                                        Some(Operate::BridgeTo(cid)) => {
                                            let mut bridges_lock = bridges.lock().await;
                                            if let Some(bri) = bridges_lock.get_mut(&cid) {
                                                match bri.waiter {
                                                    //成功桥接
                                                    Some(ref _raddr) => {
                                                        if *_raddr == addr {
                                                            return;
                                                        }
                                                        let raddr = bridges_lock
                                                            .remove(&cid)
                                                            .unwrap()
                                                            .waiter
                                                            .unwrap();
                                                        let mut table_lock = table.write().await;
                                                        table_lock
                                                            .insert(addr.clone(), raddr.clone());
                                                        table_lock.insert(raddr, addr);
                                                        println!("成功桥接");
                                                    }

                                                    //有一端连接，等待另一端
                                                    None => {
                                                        bri.waiter = Some(addr);
                                                    }
                                                }
                                            }
                                        }

                                        _ => {
                                            println_lined!("???");
                                        }
                                    }
                                }
                            },

                            Err(err) => println!("The server cannot receive a packet: {}", err),
                        }
                    }
                })
            });
        };

        let _garbage_collecter = {
            let bridges = Arc::clone(&bridges);

            tokio::spawn(async move {
                let mut interval = time::interval(Duration::from_secs(5));
                loop {
                    interval.tick().await;
                    let mut bridges_lock = bridges.lock().await;
                    bridges_lock.retain(|_, v| {
                        v.timeout += 5;
                        v.timeout <= 15
                    });
                }
            })
        };

        tokio::try_join!(
            connecter,
            //transponder,
            //garbage_collecter
        )
        .unwrap();
        Ok(())
    }
}
