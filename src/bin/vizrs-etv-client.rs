// ErsatzTV script element client: `vizrs-etv-client http://cg:7700/etv?channel=1`
// (src/etv_client.rs, DOKUMENTACJA §1).

use vizrs_cg::etv_client;

fn main() {
    let url = std::env::args().nth(1).unwrap_or_else(|| etv_client::DEFAULT_URL.to_owned());
    etv_client::run(&url);
}
