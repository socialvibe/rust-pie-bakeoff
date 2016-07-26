# Installation

```
add-apt-repository ppa:chris-lea/redis-server
sudo apt-get update
sudo apt-get install redis-server libssl-dev gcc
curl -sSf https://static.rust-lang.org/rustup.sh | sh
git clone https://github.com/socialvibe/rust-pie-bakeoff
cargo run --release
sudo vim /etc/security/limits.conf
ulimit -n 15000
```
