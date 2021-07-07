use serde::{ Serialize,Deserialize };
use bincode;
use std::{
    time::Duration,
    net::SocketAddr,
    sync::{
        Arc,Weak
    },
    fmt,
    collections::HashMap,
    ops::{ Deref, DerefMut },
    fmt::{ Display, Formatter }
};

pub type Identity=u32;
pub type Identity2=Identity;
pub type IResult<T> = Result<T,anyhow::Error>;


#[macro_export]
macro_rules! println_lined{
    ($($tt:tt)*)=>{
        println!("[{}:{}] {}",file!(),line!(),format!($($tt)*));
    }
}
/*
#[macro_export]
macro_rules! catch{
    ($expr:expr)=>{
        match $expr{
            Ok(v)=>v,
            Err(err)=>{
                println_lined!("ERR! {}",err);
                return;
            }
        }
    }
}

#[macro_export]
macro_rules! throw{
    ($expr:expr)=>{
        Err($crate::public::RuntimeError::new($expr))
    }
}
*/

#[derive(Serialize,Deserialize,Debug)]
pub enum Operate{
    Open,
    Opened(Identity),
    HelloTo(Identity),
    ConnectToMe(Identity2,Identity2), //第一个是信息管道，第二个是游戏管道
    BridgeTo(Identity2),
    Bridged(Identity2),
    OperationFailed,
    HeartBeat,
}

impl Operate{
    #[inline]
    pub fn deserialize(b:&[u8])->Option<Operate>{
        match bincode::deserialize(b){
            Ok(v)=>Some(v),
            Err(err)=>{
                dbg!(err);
                None
            }
        }
    }

    #[inline]
    pub fn serialize(&self)->Vec<u8>{
        bincode::serialize(self).unwrap()
    }
}

/*
pub struct RuntimeError(String);


impl Display for RuntimeError{
    fn fmt(f:&mut Formatter<'_>){
        write!(f,self)
    }
}
*/

pub struct MCPEInfo<'a>{
    splits:Vec<&'a [u8]>,
    pub world_name:String,
    pub game_port:u16
}


impl<'a> MCPEInfo<'a>{

    pub fn deserialize(input:&'a [u8])->Option<Self>{

        let mut counts=0usize;
        const SPLIT_INDEX:[usize;5]=[6,7,9,10,11]; //切割位点

        let sp:Vec<&'a [u8]> = input.splitn(6,|code|{
            if *code==b';'{
                if SPLIT_INDEX.contains(&counts){
                    counts+=1;
                    return true;
                }else{
                    counts+=1;
                    return false;
                }
            }else{
                return false;
            }
        }).collect();


        if sp.len()!=6{
            println_lined!("mcpe info deserialize error, buffer: {:?}", input);
            return None;
        }

        let world_name=String::from_utf8_lossy(&sp[1]).to_string();
        let game_port=String::from_utf8_lossy(&sp[5]).parse().unwrap_or(0);

        return Some(MCPEInfo{
            splits:sp,
            world_name,
            game_port
        });

    }

    pub fn serialize(&self)->Vec<u8>{
        let mut slices=Vec::with_capacity(6);
        slices.extend_from_slice(&self.splits);

        slices[1]=self.world_name.as_bytes();
        let game_port1=(self.game_port-1).to_string();
        let game_port2=self.game_port.to_string();
        slices[4]=game_port1.as_bytes();
        slices[5]=game_port2.as_bytes();

        return slices.concat();
    }
}


pub struct BridgeClient{
    udp:Weak<tokio::net::UdpSocket>,
    saddr:SocketAddr,
    baddr:SocketAddr
}


impl BridgeClient{

    #[inline]
    pub async fn bind(
        cid:Identity2,
        saddr:SocketAddr, //服务器的地址
        baddr:SocketAddr, //绑定的地址
        name_opt:Option<&'static str>,
    )->IResult<Self>{

        let udp=tokio::net::UdpSocket::bind(baddr).await?;
        Self::connect(cid,saddr,udp,name_opt).await

    }

    pub async fn connect(
        cid:Identity2,
        saddr:SocketAddr,          //服务器的地址
        udp:tokio::net::UdpSocket,
        name_opt:Option<&'static str>,
    )->IResult<Self>{
        {
            let mut buf=[0u8;512];
            let mut interval=tokio::time::interval(Duration::from_secs(1));
            let packet=Operate::BridgeTo(cid).serialize();
            let mut try_times=0u8;
            loop{
                tokio::select!{
                    //定时尝试连接
                    _=interval.tick()=>{
                        dbg!(88);
                        udp.send_to(&packet,&saddr).await?;
                        try_times+=1;
                        if try_times>=5{
                            #[derive(Debug)]
                            struct TimedOutError;
                            impl fmt::Display for TimedOutError{
                                fn fmt(&self,f:&mut fmt::Formatter<'_>)->fmt::Result{
                                    write!(f,"Out of time")
                                }
                            }
                            impl std::error::Error for TimedOutError{}

                            return Err(std::io::Error::new(
                                    std::io::ErrorKind::TimedOut,
                                    TimedOutError
                            ));
                        }
                    },

                    //监听是否连接成功
                    result=udp.recv_from(&mut buf)=>{
                        let (len,from)=match result{
                            Ok(ok)=>ok,
                            Err(err)=>{
                                println_lined!("Error: {}",err);
                                continue;
                            }
                        };
                        if from != saddr { continue; }
                        if let Some(Operate::Bridged)=Operate::deserialize(&buf[..len]){
                            break;
                        }
                    }
                }
            }
        }

        if let Some(name)=name_opt{
            println!("`{}` connected!",name);
        }

        let udp=Arc::new(udp);

        {
            let udp=Arc::clone(&udp);
            tokio::spawn(async move{
                let mut static_time=0u8;
                let mut interval=tokio::time::interval(Duration::from_secs(6));

                loop{
                    //心跳检查 break后udp被析构
                    interval.tick().await;
                    static_time+=6;
                    if static_time>10{
                        if let Err(_)=udp.send_to(&[],&saddr).await{
                            break;
                        }
                    }
                    if static_time>=20{
                        println!("回收");
                        break;
                    }
                }
            });
        }

        return Ok(
            BridgeClient{
                udp:Arc::downgrade(&udp),
                baddr:udp.local_addr().unwrap(),
                saddr,
            }
        );

    }

    pub async fn recv_from(&self,b:&mut [u8])->Option<(usize,SocketAddr)>{
        match self.udp.upgrade(){
            Some(arc)=>arc.recv_from(b).await.ok(),
            None=>None
        }
    }

    pub async fn send_to(&self,b:&[u8],d:&SocketAddr)->Option<()>{
        match self.udp.upgrade(){
            Some(arc)=>{
                arc.send_to(b,d).await.ok()?;
                Some(())
            },
            None=>None
        }
    }

    #[inline]
    pub fn saddr(&self)->&SocketAddr{
        &self.saddr
    }

    #[inline]
    pub fn baddr(&self)->&SocketAddr{
        &self.baddr
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
