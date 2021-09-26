fn main() {
    #[cfg(feature = "usdt")]
    sonde::Builder::new()
        .file("./radicle_link.d")
        .compile();
}
