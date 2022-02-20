## MCMU
MCMU是一个minecraft联机工具，MCMU-RS是Rust版本的MCMU。

所有已编译的可执行文件在executable文件夹下。

输入`./mcmu --help`来获取帮助。

可以把编译后的二进制文件分享给任何人参与测试或者游玩。

## 使用注意
- Windows不能开服
- 需要在Minecraft服务器列表添加一项`127.0.0.1:19138`才能正常加入房间。
不能添加的可以修改`external_servers.txt`手动添加。
- 测试阶段mcmu-rs提供默认服务器。后续会有变更。

## 还需要添加的功能
- [ ] 自定义服务器地址
- [ ] 背包切换插件
- [ ] 自动添加依赖minecraft服务器列表
- [ ] Windows开服
