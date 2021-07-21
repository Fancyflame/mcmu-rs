import os
import shutil


def dothis(arch, shortname, suffix=""):
    print(os.system("cargo b --release --target=" + arch))
    if not os.path.exists("executable/"+shortname):
        os.mkdir("executable/" + shortname)
    shutil.copy(
        "target/" + arch + "/release/mcmu" + suffix,
        "executable/" + shortname + "/mcmu" + suffix,
    )


dothis("aarch64-unknown-linux-musl", "arm64-linux")
dothis("x86_64-unknown-linux-musl", "amd64-linux")
dothis("x86_64-pc-windows-msvc", "amd64-windows", ".exe")

print("编译完成")
