use crate::println_lined;
use crate::public::*;
use rand::{random, Rng};
use std::{
    collections::HashMap,
    convert::TryInto,
    //io::Write,
    net::{IpAddr, SocketAddr},
    sync::{
        atomic::{AtomicBool, AtomicU32, AtomicU8, Ordering},
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
    tx: mpsc::Sender<(Identity2, Identity2)>, //两个管道的id，连接者的ip地址
    addr: SocketAddr,
    //pwd:Option<[u8;32]>
}

#[derive(Debug)]
struct Bridge {
    peer1: BridgePeer,
    peer2: BridgePeer,
    timeout: u8, //超过一定时间后销毁
}

#[derive(Debug)]
enum BridgePeer {
    Empty(IpAddr),
    Ready(SocketAddr),
}

#[derive(Debug)]
struct TableItem {
    raddr: SocketAddr,
    static_time: AtomicU8,
    should_clean: Arc<AtomicBool>,
}

impl Bridge {
    fn new(ip1: IpAddr, ip2: IpAddr) -> Self {
        Bridge {
            peer1: BridgePeer::Empty(ip1),
            peer2: BridgePeer::Empty(ip2),
            timeout: 0,
        }
    }

    fn upgrade(&mut self, addr: SocketAddr) -> IResult<()> {
        if let BridgePeer::Empty(ref ip) = self.peer1 {
            if *ip == addr.ip() {
                self.peer1 = BridgePeer::Ready(addr);
                return Ok(());
            }
        }

        if let BridgePeer::Empty(ref ip) = self.peer2 {
            if *ip == addr.ip() {
                self.peer2 = BridgePeer::Ready(addr);
                return Ok(());
            }
        }

        dbg!(&self);
        Err(anyhow::anyhow!("Cannot upgrade the bridge"))
    }

    fn connected(&self) -> bool {
        matches!(self.peer1, BridgePeer::Ready(_)) && matches!(self.peer2, BridgePeer::Ready(_))
    }

    fn finish(self) -> (SocketAddr, SocketAddr) {
        match (self.peer1, self.peer2) {
            (BridgePeer::Ready(a1), BridgePeer::Ready(a2)) => (a1, a2),
            _ => panic!("Cannot finish a bridge that has not been connected!"),
        }
    }
}

impl Drop for TableItem {
    fn drop(&mut self) {
        self.should_clean.store(true, Ordering::Relaxed);
    }
}

impl Server {
    pub async fn run(addr: SocketAddr) -> IResult<()> {
        //连接口令计数器（生成器）
        let bridge_counter = Arc::new(AtomicU32::new(random()));
        //等待连接
        let bridges = Arc::new(Mutex::new(HashMap::<Identity2, Bridge>::new()));
        //转发表
        let table = Arc::new(RwLock::new(HashMap::<SocketAddr, TableItem>::new()));

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

                                    //已经没位置了
                                    if rooms_lock.len()>=0xffffffff{
                                        return Err(anyhow::anyhow!("THE ROOM POOL IS FULL!"));
                                    }

                                    //寻找可用的id
                                    let room_id={
                                        let mut times=0u8;
                                        if rand::thread_rng().gen_range(0..1000)==0 && !rooms_lock.contains_key(&114514){
                                            //有千分之一的几率触发彩蛋
                                            114514
                                        }else{
                                            loop{
                                                let id=rand::thread_rng().gen_range(0..1000000);
                                                if !rooms_lock.contains_key(&id){
                                                    break id;
                                                }
                                                times+=1;
                                                if times==5{
                                                    fail!();
                                                    return Ok(());
                                                }
                                            }
                                        }
                                    };

                                    //已开服
                                    let (tx,mut rx)=mpsc::channel(4);
                                    println!("有房间建立：{:06}",room_id);
                                    stream.write(&Operate::Opened(room_id).serialize()).await?;
                                    rooms_lock.insert(room_id,Room{ tx, addr: haddr });
                                    drop(rooms_lock);


                                    loop{


                                        tokio::select!{


                                            //有玩家连接
                                            opt=rx.recv()=>{
                                                let (cid1,cid2)=match opt{
                                                    Some(n)=>n,
                                                    None=>return Err(anyhow::anyhow!("channel has been closed unexpectedly"))
                                                };
                                                stream.write(&Operate::ConnectToMe(cid1,cid2).serialize()).await?;
                                            },

                                            //房主有动作
                                            result=stream.read(&mut buf)=>match result{

                                                //关闭房间
                                                Err(_) | Ok(0)=>{
                                                    rooms.write().await.remove(&room_id);
                                                    println!("房间已关闭：{:06}",room_id);
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

                                            let mut bridges_lock=bridges.lock().await;
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
                                            bridges_lock.insert(cid1,Bridge::new(haddr.ip(),room.addr.ip()));
                                            bridges_lock.insert(cid2,Bridge::new(haddr.ip(),room.addr.ip()));
                                            room.tx.send((cid1,cid2)).await.unwrap(); //把连接信息发给房主
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

            let p = || {
                let socket = socket.clone();
                let bridges = Arc::clone(&bridges);
                let table = Arc::clone(&table);
                tokio::spawn(log_i_result("UDP转发", async move {
                    let mut buf = [0u8; 1500];
                    loop {
                        let (len, addr) = match socket.recv_from(&mut buf).await {
                            Ok(n) => n,
                            Err(_) => continue,
                        };

                        //检查是否可连接
                        if buf[0] == CommunicatePacket::CONNECT {
                            let mut bridges_lock = bridges.lock().await;
                            let cid = Identity2::from_be_bytes(buf[1..len].try_into().unwrap());

                            if let Some(bri) = bridges_lock.get_mut(&cid) {
                                //地址不对应
                                if let Err(_err) = bri.upgrade(addr) {
                                    println_lined!("{:#?}", *table.read().await);
                                    continue;
                                }

                                if !bri.connected() {
                                    continue;
                                }

                                let (a1, a2) = bridges_lock.remove(&cid).unwrap().finish();

                                println!("正在桥接");

                                let mut table_lock = table.write().await;
                                let arc = Arc::new(AtomicBool::new(false));

                                table_lock.insert(
                                    a1.clone(),
                                    TableItem {
                                        raddr: a2.clone(),
                                        static_time: AtomicU8::new(0),
                                        should_clean: arc.clone(),
                                    },
                                );

                                table_lock.insert(
                                    a2.clone(),
                                    TableItem {
                                        raddr: a1.clone(),
                                        static_time: AtomicU8::new(0),
                                        should_clean: arc,
                                    },
                                );

                                buf[0] = CommunicatePacket::ACK;
                                socket.send_to(&buf[..5], a1).await.unwrap();
                                socket.send_to(&buf[..5], a2).await.unwrap();

                                println!("成功桥接");
                                continue;
                            }
                        }

                        let table_read = table.read().await;

                        match table_read.get(&addr) {
                            //转发
                            Some(TableItem {
                                ref raddr,
                                ref static_time,
                                ..
                            }) => {
                                static_time.store(0, Ordering::Relaxed);
                                match buf[0] {
                                    CommunicatePacket::DATA => {
                                        socket.send_to(&buf[..len], raddr).await.unwrap();
                                    }

                                    CommunicatePacket::CONNECT => {
                                        println!("CONN");
                                        buf[0] = CommunicatePacket::ACK;
                                        socket.send_to(&buf[..len], addr).await.unwrap();
                                    }

                                    CommunicatePacket::HEARTBEAT => {}
                                    _ => {
                                        let l = if len < 10 { len } else { 10 };
                                        println_lined!(
                                            "Malformed or invalid packet: {:?}. Ignored.",
                                            &buf[..l]
                                        );
                                    }
                                }
                            }

                            //前面已经桥接过了。无论如何在这里出现数据包都应该回复关闭。
                            None => {
                                println_lined!(
                                    "???: {:?}",
                                    &buf[..if len < 10 { len } else { 10 }]
                                );
                                const CLOSE: [u8; 1] = [CommunicatePacket::CLOSE];
                                socket.send_to(&CLOSE, addr).await.unwrap();
                            }
                        }
                    }
                    //return IResult::Ok(());
                }));
            };

            for _ in 0..4 {
                p();
            }
        };

        let _garbage_collecter = {
            let bridges = bridges.clone();
            let table = table.clone();

            tokio::spawn(async move {
                let mut interval = time::interval(Duration::from_secs(5));
                loop {
                    interval.tick().await;

                    //先清理未连接的
                    let mut bridges_lock = bridges.lock().await;
                    bridges_lock.retain(|_, v| {
                        v.timeout += 5;
                        v.timeout < 20
                    });

                    //再清理死亡的
                    let mut table_lock = table.write().await;
                    table_lock.retain(|_, v| {
                        if v.should_clean.load(Ordering::Relaxed) {
                            return false;
                        }

                        let mut val = v.static_time.load(Ordering::Relaxed);
                        val += 5;
                        v.static_time.store(val, Ordering::Relaxed);
                        if val < 10 {
                            true
                        } else {
                            println!("回收");
                            false
                        }
                    });
                }
            })
        };

        /*let check_lock = || {
            let bridges = bridges.clone();
            let table = table.clone();
            tokio::spawn(async move {
                loop {
                    tokio::time::sleep(Duration::from_secs(4)).await;
                    if let Err(_err) = tokio::time::timeout(Duration::from_secs(1), async {
                        bridges.lock().await;
                        table.write().await;
                    })
                    .await
                    {
                        println!("服务器发生死锁！");
                    }
                }
            });
        };
        check_lock();*/

        connecter.await??;
        Ok(())
    }
}
