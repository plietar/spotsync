extern crate env_logger;
extern crate getopts;
extern crate hyper;
extern crate librespot;
extern crate rustc_serialize;
extern crate libmsntp;
extern crate libc;
#[macro_use]
extern crate log;

use std::process::exit;
use std::env;
use std::clone::Clone;
use std::thread;
use std::sync::Mutex;
use std::ffi::CString;
use std::io::Read;

use libc::timeval;

use libmsntp::msntp_start_server;
use libmsntp::msntp_serve;
use libmsntp::msntp_get_offset;

use hyper::Client;
use hyper::Server;
use hyper::server::Request;
use hyper::server::Response;
use hyper::header::Connection;
use rustc_serialize::json;

use librespot::player::{Player, PlayStatus};
use librespot::spirc::SpircManager;
use librespot::util::now_ms;
use librespot::util::SpotifyId;
use librespot::main_helper;

fn usage(program: &str, opts: &getopts::Options) -> String {
    let brief = format!("Usage: {} [options]", program);
    format!("{}", opts.usage(&brief))
}

#[derive(Debug, Clone, RustcDecodable, RustcEncodable)]
struct SyncStatus {
    playing: bool,
    track: String,
    start_time: i64,
}

fn sync_thread(player: Player, remote: &str) {
    let ntp_host = CString::new(remote).unwrap();
    let http_host = format!("http://{}:5000", remote);

    let mut state = SyncStatus {
        playing: false,
        track: String::new(),
        start_time: 0,
    };
    let mut sync_offset;

    let mut drift = 0;
    loop {
        debug!("starting sync ...");

        sync_offset = unsafe {
            let mut offset : timeval = std::mem::zeroed();
            msntp_get_offset(ntp_host.as_ptr(), 5001, &mut offset as *mut _);
            offset.tv_sec as i64 * 1000 + offset.tv_usec as i64
        };

        debug!("NTP offset: {}", sync_offset);

        let new_state = get_sync_status(&http_host);
        let remote_position = (now_ms() - (new_state.start_time) + sync_offset) as u32;

        if new_state.track != state.track && !new_state.track.is_empty() {
            debug!("player state: {:?}", new_state);
            player.load(SpotifyId::from_base16(&new_state.track), new_state.playing, remote_position);
        } else {
            match (state.playing, new_state.playing) {
                (false, true) => {
                    player.seek_at(remote_position, now_ms());
                    player.play();
                }
                (true, false) => {
                    player.pause();
                }
                (true, true) => {
                    let (local_pos, measured_at) = player.state().position();

                    let local_position = (local_pos as i64 + now_ms() - measured_at) as u32;

                    let diff = local_position as i64 - remote_position as i64;
                    drift = drift / 2 + diff / 2;


                    debug!("player offset: local={} remote={} diff={} drift={}", local_position, remote_position, diff, drift);
                    if drift.abs() > 20 {
                        let seek = std::cmp::max(0, remote_position as i64) as u32;

                        player.seek_at(seek, now_ms());
                        drift = 0;
                    }

                }
                (false, false) => ()
            }
        }

        state = new_state;

        std::thread::sleep(std::time::Duration::from_millis(500));
    }
}

fn get_sync_status(url: &str) -> SyncStatus {
    let client = Client::new();

    let mut res = client.get(url)
        .header(Connection::close())
        .send().unwrap();

    let mut body = String::new();
    res.read_to_string(&mut body).unwrap();

    json::decode(&body).unwrap()
}

fn http_server(player: Player) {
    let player = Mutex::new(player);
    Server::http("0.0.0.0:5000").unwrap().handle(move |_: Request, res: Response| {
        let state = player.lock().unwrap().state();
        let status = SyncStatus {
            playing: state.status == PlayStatus::kPlayStatusPlay,
            track: state.track.map(|t| t.to_base16()).unwrap_or(String::new()),
            start_time: state.position_measured_at - state.position_ms as i64,
        };
        println!("{:?}", status);
        res.send(json::encode(&status).unwrap().as_bytes()).unwrap();
    }).unwrap();
}

fn ntp_server() {
    unsafe {
        let ret = msntp_start_server(5001);
        assert_eq!(ret, 0);
        loop {
            msntp_serve();
        }
    }
}

fn main() {
    if env::var("RUST_LOG").is_err() {
        env::set_var("RUST_LOG", "info,spotsync=trace,librespot=trace")
    }
    env_logger::init().unwrap();

    let mut opts = getopts::Options::new();
    main_helper::add_session_arguments(&mut opts);
    main_helper::add_authentication_arguments(&mut opts);
    main_helper::add_player_arguments(&mut opts);

    opts.optflag("", "master", "Master mode.");

    let args: Vec<String> = std::env::args().collect();
    let matches = match opts.parse(&args[1..]) {
        Ok(m) => m,
        Err(f) => {
            error!("Error: {}\n{}", f.to_string(), usage(&args[0], &opts));
            exit(1)
        }
    };

    let session = main_helper::create_session(&matches);
    let credentials = main_helper::get_credentials(&session, &matches);
    session.login(credentials).unwrap();

    let player = main_helper::create_player(&session, &matches);

    if matches.opt_present("master") {
        let spirc = SpircManager::new(session.clone(), player.clone());

        thread::spawn(move || http_server(player));
        thread::spawn(move || ntp_server());
        thread::spawn(move || spirc.run());
    } else {
        let remote = matches.free.first().expect("missing master address").clone();
        thread::spawn(move || sync_thread(player, &remote));
    }

    loop {
        session.poll();
    }
}
