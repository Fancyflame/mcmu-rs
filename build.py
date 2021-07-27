import os
import shutil
import sys


def dothis(arch, shortname, suffix=""):

    global newexec
    if len(sys.argv) >= 2:
        newexec = sys.argv[1] == "all"  # 是否为第一次使用该脚本
    else:
        newexec = False

    target = "target/" + arch + "/release"
    output = "executable/" + shortname

    if os.path.exists(target) and not os.path.exists(output) and not newexec:
        print(arch + "架构可能无法编译。若要加入编译请使用all参数")
        return

    ret = os.system("cargo b --release --target=" + arch)

    if ret != 0:
        print("无法编译" + arch + "架构。")
        return

    if not os.path.exists(output):
        os.mkdir(output)

    shutil.copy(
        target + "/mcmu" + suffix, output + "/mcmu" + suffix,
    )

    print(arch + "架构编译完成")


dothis("aarch64-unknown-linux-musl", "arm64-linux")
dothis("x86_64-unknown-linux-musl", "amd64-linux")
dothis("x86_64-pc-windows-msvc", "amd64-windows", ".exe")
dothis("aarch64-apple-ios", "arm64-ios")
dothis("aarch64-apple-darwin", "arm64-macos")
dothis("x86_64-apple-darwin", "amd64-macos")
