pub(crate) fn prepend_icon_paths(theme: &gtk::IconTheme, ours: &[std::path::PathBuf]) {
    let mut combined: Vec<std::path::PathBuf> = ours.to_vec();
    for p in theme.search_path() {
        if !combined.contains(&p) {
            combined.push(p);
        }
    }
    let refs: Vec<&std::path::Path> = combined.iter().map(|p| p.as_path()).collect();
    theme.set_search_path(&refs);
}
