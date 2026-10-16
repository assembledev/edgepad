use super::*;

#[test]
fn edge_widths_label_prints_all_zones() {
    assert_eq!(
        edge_widths_label(EdgeWidths {
            left: 0.0,
            right: 0.1,
            top: 0.2,
            bottom: 0.3,
        }),
        "left=0.000 right=0.100 top=0.200 bottom=0.300"
    );
}
