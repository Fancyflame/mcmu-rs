import os
import shutil


def dothis(arch, shortname, suffix=""):
    exit_code=os.system("cargo b --release --target=" + arch)
    if exit_code!=0:
        print("编译"+arch+"失败")
        return
    if not os.path.exists("executable/"+shortname):
        os.mkdir("executable/" + shortname)
    shutil.copy(
        "target/" + arch + "/release/mcmu" + suffix,
        "executable/" + shortname + "/mcmu" + suffix,
    )


dothis("aarch64-unknown-linux-musl", "arm64-linux")
dothis("x86_64-unknown-linux-musl", "amd64-linux")
dothis("x86_64-pc-windows-msvc", "amd64-windows", ".exe")
dothis("aarch64-apple-ios", "arm64-ios")
dothis("aarch64-apple-darwin", "arm64-macos")
dothis("x86_64-apple-darwin", "amd64-macos")

print("编译完成")
