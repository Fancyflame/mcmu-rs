use std::convert::TryFrom;
use std::net::SocketAddr;
use std::time::Duration;
#[macro_use]
extern crate lazy_static;
#[macro_use]
extern crate clap;

mod client;
mod host;
mod public;
mod server;

#[tokio::main]
async fn main() {
    let srv = SocketAddr::new(/*[123, 207, 9, 213]*/ [127, 0, 0, 1].into(), 27979);
    let matches = clap_app!(
        MCMU=>
        (version:"1.0")
        (author:"FancyFlame <fancyflame@163.com>")
        (about:"一个Minecraft基岩版的联机工具")
        (@subcommand s=>
            (about:"启动服务器")
            (@arg ADDR: +required "服务器监听地址")
        )
        (@subcommand o=>
            (about:"开一个联机房间，供其他人加入")
        )
        (@subcommand j=>
            (about:"加入其他人的房间")
            (@arg ROOM_NUM: +required "房间号")
        )
    )
    .get_matches();

    let result = match matches.subcommand() {
        ("s", Some(subm)) => match subm.value_of("ADDR").unwrap().parse::<SocketAddr>() {
            Ok(SocketAddr::V6(_)) => {
                println!("只支持IPv4地址");
                Ok(())
            }
            Ok(SocketAddr::V4(a)) => {
                println!("服务器开始运行");
                server::Server::run(a.into()).await
            }
            Err(_) => {
                println!("你输的是什么玩意，IPv4地址格式是xxx.xxx.xxx.xxx:xxx");
                Ok(())
            }
        },

        ("o", Some(_)) => {
            println!("测试版，服务器地址已自动填入");
            host::Host::run(srv).await
        }

        ("j", Some(subm)) => match subm
            .value_of("ROOM_NUM")
            .unwrap()
            .parse::<public::Identity2>()
        {
            Ok(num) => {
                println!("测试版，服务器地址已自动填入");
                client::Client::run(srv, num).await
            }
            _ => {
                println!("输入的房间号无效");
                Ok(())
            }
        },

        _ => {
            println!("请使用 --help 参数来显示帮助");
            Ok(())
        }
    };

    if let Err(err) = result {
        println!("错误：{}", err);
    };

    /*println!("Hello, world!");
    let addr:SocketAddr="127.0.0.1:12233".parse().unwrap();
    let srv=tokio::spawn(server::Server::run(addr.clone()));

    tokio::time::sleep(Duration::from_millis(500u64)).await;
    let host=tokio::spawn(host::Host::run(addr.clone()));

    tokio::time::sleep(Duration::from_millis(500u64)).await;
    let client=tokio::spawn(client::Client::run(addr.clone(),100));
    match tokio::try_join!(srv,host,client){
        Ok(_)=>{},
        Err(err)=>{
            println!("main error: {}",err);
        }
    }*/
}

/*fn main() {
    let rt_ = std::sync::Arc::new(tokio::runtime::Runtime::new().unwrap());
    let rt = rt_.clone();
    rt_.block_on(async move {
        const PORT: u16 = 17792;
        rt.spawn(server::Server::run(SocketAddr::new(
            [0, 0, 0, 0].into(),
            PORT,
        )));
        tokio::time::sleep(Duration::from_millis(500)).await;
        println!("服务器已启动");

        rt.spawn(host::Host::run(SocketAddr::new(
            [127, 0, 0, 1].into(),
            PORT,
        )));
        tokio::time::sleep(Duration::from_millis(500)).await;
        println!("已启动房间");

        rt.spawn(client::Client::run(
            SocketAddr::new([127, 0, 0, 1].into(), PORT),
            123456,
        ));
        tokio::time::sleep(Duration::from_millis(10000)).await;
        println!("已启动玩家");
        /*match tokio::try_join!(srv, host) {
            Ok(_) => {}
            Err(err) => {
                println!("main error: {}", err);
            }
        }*/
    });
}*/
