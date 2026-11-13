use exynos_loke_rescue::mock::MockLokeDevice;
use exynos_loke_rescue::ui::{
    create_flash_progress, format_bytes, render_card, render_pit_table, render_table,
};

#[test]
fn test_format_bytes() {
    assert_eq!(format_bytes(0), "0 B");
    assert_eq!(format_bytes(500), "500 B");
    assert_eq!(format_bytes(1024), "1.00 KiB");
    assert_eq!(format_bytes(1024 * 512), "512.00 KiB");
    assert_eq!(format_bytes(1024 * 1024), "1.00 MiB");
    assert_eq!(format_bytes(36 * 1024 * 1024), "36.00 MiB");
    assert_eq!(format_bytes(1024 * 1024 * 1024), "1.00 GiB");
    assert_eq!(format_bytes(5 * 1024 * 1024 * 1024), "5.00 GiB");
}

#[test]
fn test_render_table_and_card_no_panic() {
    let headers = ["ID", "PARTITION", "SIZE"];
    let rows = vec![
        vec!["1".into(), "BOOT".into(), "36.00 MiB".into()],
        vec!["2".into(), "RECOVERY".into(), "53.00 MiB".into()],
    ];
    render_table("TEST PARTITIONS", &headers, &rows);

    let card_items = [
        ("Device", "Samsung Galaxy A21s"),
        ("Protocol", "Odin v3/v4"),
        ("Status", "Online"),
    ];
    render_card("TEST CARD", &card_items);
}

#[test]
fn test_render_pit_table_mock_no_panic() {
    let mock = MockLokeDevice::new();
    render_pit_table(&mock.pit_table);
    assert_eq!(mock.pit_table.entry_count, 6);
}

#[test]
fn test_progress_bar_creation() {
    let pb = create_flash_progress("SUPER", 5 * 1024 * 1024 * 1024);
    assert_eq!(pb.length(), Some(5 * 1024 * 1024 * 1024));
}
