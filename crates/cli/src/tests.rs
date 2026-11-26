use super::*;

#[test]
fn confirmation_answers() {
    assert_eq!(parse_answer("y\n", 5), Answer::Selected);
    assert_eq!(parse_answer(" YES ", 5), Answer::Selected);
    for no in ["", "n", "no", "maybe", "0", "9", "1,x", "-1"] {
        assert_eq!(parse_answer(no, 5), Answer::No, "{no:?}");
    }
    assert_eq!(parse_answer("1,3", 5), Answer::Numbers(vec![1, 3]));
    assert_eq!(parse_answer("2 4", 5), Answer::Numbers(vec![2, 4]));
    assert_eq!(parse_answer("5", 5), Answer::Numbers(vec![5]));
}

#[test]
fn paths_are_shortened_relative_to_home() {
    assert_eq!(
        short(
            Path::new("/home/u/AppImages/a.AppImage"),
            Path::new("/home/u")
        ),
        "~/AppImages/a.AppImage"
    );
    assert_eq!(
        short(Path::new("/opt/a.AppImage"), Path::new("/home/u")),
        "/opt/a.AppImage"
    );
}
