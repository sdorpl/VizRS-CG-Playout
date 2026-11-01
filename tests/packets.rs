// ETV packets byte for byte as the Node.js build writes them (tests/vectors/frames.json).

mod common;

use vizrs_cg::json::Value;
use vizrs_cg::packets::{Rect, crop, picture_packet, repeat_packet};

use common::{assert_bytes, num, paint, stored_bytes, vectors};

#[test]
fn picture_packets_match_node() {
    let v = vectors("frames");
    let rects = v.get("rects").and_then(Value::as_array).unwrap();
    let packets = v.get("packets").and_then(Value::as_array).unwrap();
    for (case, packet) in rects.iter().zip(packets) {
        let (width, _, rgba) = paint(case.get("picture").unwrap());
        let boxes: Vec<Rect> = case
            .get("rects")
            .and_then(Value::as_array)
            .unwrap()
            .iter()
            .map(|r| {
                let (x, y, w, h) = (num(r, "x"), num(r, "y"), num(r, "w"), num(r, "h"));
                Rect { x, y, w, h, pixels: crop(&rgba, width, (x, y, w, h)) }
            })
            .collect();
        assert_bytes(&picture_packet(&boxes), packet.get("packet").unwrap(), &case.get("name").unwrap().to_string());
    }
}

#[test]
fn repeat_packets_match_node() {
    let v = vectors("frames");
    for case in v.get("repeats").and_then(Value::as_array).unwrap() {
        let frames = case.get("frames").and_then(Value::as_f64).unwrap() as u64;
        assert_eq!(repeat_packet(frames), stored_bytes(case.get("packet").unwrap()).unwrap(), "{frames}");
    }
}
