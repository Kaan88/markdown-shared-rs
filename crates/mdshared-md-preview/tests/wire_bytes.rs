use mdshared_md_preview::wire::{
    MAGIC, MAX_MESSAGE, MAX_SIDE, Request, Update, read_request, read_update, write_request,
    write_update,
};

#[test]
fn preview_seven_keeps_its_magic_limits_and_frame_bytes() {
    assert_eq!(MAGIC, b"MDVPREVIEW7\n");
    assert_eq!(MAX_MESSAGE, 268_435_520);
    assert_eq!(MAX_SIDE, 8192);

    let req = Request::Follow {
        line: 0x1234,
        column: 0x5678,
    };
    let expected = b"\x03\0\0\0\x08\0\0\0\x34\x12\0\0\x78\x56\0\0";
    let mut bytes = Vec::new();
    write_request(&mut bytes, &req).unwrap();
    assert_eq!(bytes, expected);
    assert_eq!(read_request(&mut &expected[..]).unwrap(), Some(req));

    let update = Update::ack(0x1234);
    let expected = b"\x10\0\0\0\x48\0\0\0\
        \0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\
        \0\0\0\0\x01\0\0\0\0\0\0\0\0\0\0\0\
        \0\0\0\0\x34\x12\0\0\0\0\0\0\0\0\0\0\
        \0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0";
    bytes.clear();
    write_update(&mut bytes, &update).unwrap();
    assert_eq!(bytes, expected);
    assert_eq!(read_update(&mut &expected[..]).unwrap(), Some(update));
}
