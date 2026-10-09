use super::*;

#[test]
fn row_request_outer_arrays_are_not_documented_objects_in_both_authority_modes() {
    let admitted: Vec<_> = [
        (
            Operation::Get,
            br#"["t",{"type":"integer","value":"1"}]"#.as_slice(),
        ),
        (Operation::Page, br#"["t",null,1]"#),
        (
            Operation::Batch,
            br#"["t",[{"op":"delete","key":{"type":"integer","value":"1"}}]]"#,
        ),
    ]
    .into_iter()
    .map(|(kind, bytes)| {
        (
            validate_row_request(kind, bytes).is_ok(),
            validate_user_row_request(kind, bytes).is_ok(),
        )
    })
    .collect();
    assert_eq!(
        admitted,
        vec![(false, false); 3],
        "outer objects required in both modes"
    );
}
