//! Character card files (PNG and JSON) from any source. The parser must
//! return a card or an error, and never panic or hang.
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|bytes: &[u8]| {
    let _ = unstrung_core::card::parse_card(bytes);
});
