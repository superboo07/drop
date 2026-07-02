use log::warn;

use crate::external_open::open_externally;

pub fn webbrowser_open<T: AsRef<str>>(url: T) {
    let result = open_externally(url.as_ref(), || webbrowser::open(url.as_ref()));
    if let Err(e) = result {
        warn!(
            "Could not open web browser to url {} with error {}",
            url.as_ref(),
            e
        );
    };
}
