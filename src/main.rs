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
//mod ring_buffer;

#[tokio::main]
async fn main() {
    let srv = SocketAddr::new(
        [39, 108, 179, 179].into(),
        //[127, 0, 0, 1].into(),
        27979,
    );
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
        (@subcommand t=>
            (about:"运行测试")
        )
    )
    .get_matches();

    lazy_static::initialize(&public::LOCALADDR);
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

        ("j", Some(subm)) => {
            let room = subm.value_of("ROOM_NUM").unwrap();
            if room.len() != 6 {
                println!("请向房主询问并输入6位数字房间号");
                Ok(())
            } else {
                match room.parse::<public::Identity2>() {
                    Ok(num) => {
                        println!("测试版，服务器地址已自动填入");
                        client::Client::run(srv, num).await
                    }
                    _ => {
                        println!("输入的房间号无效");
                        Ok(())
                    }
                }
            }
        }

        ("t", Some(_)) => {
            println!("测试版，服务器地址已自动填入");
            let data=hex::decode("1c0000000000d1fe2180f5403287a99bb200ffff00fefefefefdfdfdfd12345678005d4d4350453b46616e6379466c616d65583b3435363b312e31372e32302e32323b313b383b31333738363935373235393131343130313130353be68891e79a84e4b896e7958c3b43726561746976653b313b35343339323b35343339333b")
                .unwrap();
            let info = public::MCPEInfo::deserialize(&data).unwrap();
            println!(
                "房间名称：{}。游戏端口：{}",
                info.world_name, info.game_port
            );
            Ok(())
        }

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
