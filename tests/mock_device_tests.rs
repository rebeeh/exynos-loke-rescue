use exynos_loke_rescue::loke_proto::{
    ControlFrame, ProtocolTier, SessionEndSubcmd, HANDSHAKE_REQUEST, HANDSHAKE_RESPONSE,
};
use exynos_loke_rescue::mock::{MockDeviceState, MockLokeDevice};

#[test]
fn test_mock_device_handshake_flow() {
    let mut device = MockLokeDevice::new();
    assert_eq!(device.state, MockDeviceState::Disconnected);

    // Invalid handshake request should fail
    assert!(device.handle_handshake(b"HELO").is_err());

    // Valid "ODIN" handshake should return "LOKE"
    let resp = device.handle_handshake(HANDSHAKE_REQUEST).unwrap();
    assert_eq!(&resp, HANDSHAKE_RESPONSE);
    assert_eq!(device.state, MockDeviceState::HandshakeDone);
}

#[test]
fn test_mock_device_handshake_fault_injection() {
    let mut device = MockLokeDevice::new();
    device.faults.reject_handshake = true;
    assert!(device.handle_handshake(HANDSHAKE_REQUEST).is_err());
}

#[test]
fn test_mock_device_session_setup_and_param_flag() {
    let mut device = MockLokeDevice::new();
    let _ = device.handle_handshake(HANDSHAKE_REQUEST).unwrap();

    assert_eq!(device.param_update_flag, 0x00);

    // Begin flashing session
    let setup = ControlFrame::session_setup_init(ProtocolTier::OdinV3);
    let ack = device.handle_control_frame(&setup).unwrap();
    assert_eq!(ack.cmd_id, 0x64);
    assert!(ack.is_success());
    assert_eq!(device.param_update_flag, 0x01); // Param flag must be set to 0x01

    // Close flashing session
    let end = ControlFrame::session_end(SessionEndSubcmd::RebootNormal);
    let end_ack = device.handle_control_frame(&end).unwrap();
    assert_eq!(end_ack.cmd_id, 0x67);
    assert!(end_ack.is_success());
    assert_eq!(device.param_update_flag, 0x00); // Param flag must be committed back to 0x00
}

#[test]
fn test_mock_device_nand_hardware_erase() {
    let mut device = MockLokeDevice::new();
    let _ = device.handle_handshake(HANDSHAKE_REQUEST).unwrap();
    let _ = device
        .handle_control_frame(&ControlFrame::session_setup_init(ProtocolTier::OdinV3))
        .unwrap();

    assert!(!device.userdata_erased);

    let erase_frame = ControlFrame::nand_erase();
    let ack = device.handle_control_frame(&erase_frame).unwrap();

    assert_eq!(ack.cmd_id, 0x64);
    assert!(ack.is_success());
    assert_eq!(ack.payload(), 0x0010_0000); // 1,048,576 erased sectors
    assert!(device.userdata_erased);
}

#[test]
fn test_mock_device_partition_flashing_and_chunks() {
    let mut device = MockLokeDevice::new();
    let _ = device.handle_handshake(HANDSHAKE_REQUEST).unwrap();
    let _ = device
        .handle_control_frame(&ControlFrame::session_setup_init(ProtocolTier::OdinV3))
        .unwrap();

    let boot_id = 20; // BOOT
    let part_size = 256 * 1024; // 256 KiB
    let total_chunks = 2;

    let start_frame = ControlFrame::file_transfer_start(boot_id, part_size, total_chunks);
    let start_ack = device.handle_control_frame(&start_frame).unwrap();
    assert!(start_ack.is_success());

    // Stream 2 chunks of 128 KiB
    let chunk = vec![0xAAu8; 128 * 1024];
    let ack1 = device.handle_chunk(&chunk).unwrap();
    assert_eq!(ack1.payload(), 1);

    let ack2 = device.handle_chunk(&chunk).unwrap();
    assert_eq!(ack2.payload(), 2);

    let end_frame = ControlFrame::file_transfer_end(boot_id);
    let end_ack = device.handle_control_frame(&end_frame).unwrap();
    assert!(end_ack.is_success());

    assert_eq!(device.flashed_partitions.get("BOOT"), Some(&(256 * 1024)));
}

#[test]
fn test_mock_device_vaultkeeper_prenormal_lock_rejection() {
    let mut device = MockLokeDevice::new();
    device.faults.vaultkeeper_prenormal_lock = true;

    let _ = device.handle_handshake(HANDSHAKE_REQUEST).unwrap();
    let _ = device
        .handle_control_frame(&ControlFrame::session_setup_init(ProtocolTier::OdinV3))
        .unwrap();

    let start_frame = ControlFrame::file_transfer_start(22, 1024, 1);
    // VaultKeeper prenormal rejection returns status < 0 which LokeAck::parse converts to LokeFailure
    let res = device.handle_control_frame(&start_frame);
    assert!(res.is_err());
    let err_msg = res.unwrap_err().to_string();
    assert!(err_msg.contains("LokeFailure") || err_msg.contains("-1"));
}
