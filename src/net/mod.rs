pub fn fetch_html(url: &str) -> Result<String, ureq::Error> {
    let mut response = ureq::get(url).call()?;
    response.body_mut().read_to_string()
}
