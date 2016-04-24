extern crate gcc;

fn main() {
    gcc::Config::new()
        .file("libmsntp/main.c").file("libmsntp/unix.c")
        .file("libmsntp/internet.c").file("libmsntp/socket.c")
        .file("libmsntp/timing.c").file("libmsntp/libmsntp.c")
        .compile("libmsntp.a");
}
