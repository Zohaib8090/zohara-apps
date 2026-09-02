// zohara-update -- TODO: port from the PyQt5 version at
// zohara/zohara-profile/airootfs/usr/local/bin/zohara-update
//
// Will mirror the four UpdatePanels:
//   1. Zohara OS updates  -> pacman -Syu zohara-* + check via zohara.db
//   2. System & Apps      -> pacman -Syu
//   3. Kernel             -> pacman -Syu linux-zen linux-zen-headers
//   4. Driver & Firmware  -> pacman -Syu mesa, vulkan-*, nvidia-*, firmware
fn main() {
    eprintln!("zohara-update: not yet ported (TODO)");
    std::process::exit(1);
}
