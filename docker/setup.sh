# Exit script on error
set -e

apt update && apt -y upgrade

apt install -y --no-install-recommends --no-install-suggests ca-certificates git 

# Build from HEAD
git clone https://github.com/jmccl/RustGallery.git

# Setup for nginx: https://nginx.org/en/linux_packages.html
apt install -y curl gnupg2 ca-certificates lsb-release debian-archive-keyring
curl https://nginx.org/keys/nginx_signing.key | gpg --dearmor | tee /usr/share/keyrings/nginx-archive-keyring.gpg >/dev/null
echo "deb [signed-by=/usr/share/keyrings/nginx-archive-keyring.gpg] http://nginx.org/packages/debian `lsb_release -cs` nginx" | tee /etc/apt/sources.list.d/nginx.list
apt install -y nginx

apt install -y --no-install-recommends --no-install-suggests build-essential libclang-dev libssl-dev pkg-config grep gawk gnupg2 sed

cd RustGallery && ./build