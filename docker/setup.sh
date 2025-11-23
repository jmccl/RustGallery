# Exit script on error
set -e

apt update && apt -y upgrade

apt install -y --no-install-recommends --no-install-suggests ca-certificates git 

# Build from HEAD
git clone https://github.com/jmccl/RustGallery.git

apt install -y nginx

apt install -y --no-install-recommends --no-install-suggests build-essential libclang-dev \
                        libssl-dev libpcre2-dev libz-dev pkg-config grep gawk gnupg2 sed

cd RustGallery && ./build