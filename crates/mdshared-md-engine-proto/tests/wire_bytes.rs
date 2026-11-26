use mdshared_md_engine_proto::{
    KIND_SVG, Reply, Request, read_reply, read_request, write_reply, write_request,
};

#[test]
fn the_versionless_request_and_reply_keep_their_original_bytes() {
    let request = Request {
        kind: KIND_SVG,
        payload: b"<svg/>".to_vec(),
    };
    let expected = b"\x01\0\0\0\x06\0\0\0<svg/>";
    let mut bytes = Vec::new();
    write_request(&mut bytes, &request).unwrap();
    assert_eq!(bytes, expected);
    let decoded = read_request(&mut &expected[..]).unwrap().unwrap();
    assert_eq!(
        (decoded.kind, decoded.payload),
        (request.kind, request.payload)
    );

    let reply = Reply::ok_with_baseline(1, 2, 1, vec![1, 2, 3, 255, 4, 5, 6, 128]);
    let expected =
        b"\0\0\0\0\x01\0\0\0\x02\0\0\0\x01\0\0\0\x08\0\0\0\0\0\0\0\x01\x02\x03\xff\x04\x05\x06\x80";
    bytes.clear();
    write_reply(&mut bytes, &reply).unwrap();
    assert_eq!(bytes, expected);
    let decoded = read_reply(&mut &expected[..]).unwrap();
    assert_eq!(
        (
            decoded.status,
            decoded.width,
            decoded.height,
            decoded.baseline,
            decoded.rgba
        ),
        (
            reply.status,
            reply.width,
            reply.height,
            reply.baseline,
            reply.rgba
        )
    );
}
