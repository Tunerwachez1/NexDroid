sudo ufw allow in on waydroid0 proto udp from any port 68 to any port 67
sudo ufw allow in on waydroid0 to 192.168.240.1 port 53 proto udp
sudo ufw allow in on waydroid0 to 192.168.240.1 port 53 proto tcp
sudo ufw route allow in on waydroid0 out on enp3s0 from 192.168.240.0/24
