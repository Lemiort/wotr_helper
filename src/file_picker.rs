// Cross-platform file picker helpers. On wasm we create a hidden <input type=file> and read bytes;
// on native the functions are no-ops (native uses rfd::FileDialog directly).

#[cfg(target_arch = "wasm32")]
mod web {
    use js_sys::Uint8Array;
    use once_cell::sync::Lazy;
    use std::sync::Mutex;
    use wasm_bindgen::closure::Closure;
    use wasm_bindgen::JsCast;
    use wasm_bindgen::JsValue;
    use web_sys::{FileReader, HtmlInputElement};

    static SELECTED_IMAGE: Lazy<Mutex<Option<(Vec<u8>, String)>>> = Lazy::new(|| Mutex::new(None));

    pub fn open_image_picker() {
        // Debug: log when picker is invoked (helps detect stale builds / service worker cache)
        web_sys::console::log_1(&"file_picker: open_image_picker called".into());
        let window = match web_sys::window() { Some(w) => w, None => return };
        let document = match window.document() { Some(d) => d, None => return };

        // Create an input element and keep it off-screen instead of display:none (some browsers block clicks on display:none)
        let input = match document.create_element("input") {
            Ok(el) => el,
            Err(_) => return,
        };
        let input = match input.dyn_into::<HtmlInputElement>() {
            Ok(i) => i,
            Err(_) => return,
        };

        input.set_type("file");
        input.set_accept("image/png,image/jpeg");
        let _ = input.set_attribute("style", "position: fixed; left: -9999px; width: 1px; height: 1px; opacity: 0;");

        // Append to body so click is allowed
        if let Some(body) = document.body() {
            let _ = body.append_child(&input);
        }

        // onChange handler: read first file into bytes and store it with filename
        let onchange = Closure::wrap(Box::new(move |ev: web_sys::Event| {
            let input = match ev.target().and_then(|t| t.dyn_into::<HtmlInputElement>().ok()) {
                Some(i) => i,
                None => return,
            };
            if let Some(files) = input.files() {
                if let Some(file) = files.get(0) {
                    let fr = FileReader::new().unwrap();
                    let fr2 = fr.clone();
                    let name = file.name();
                    let onload = Closure::once(Box::new(move |_e: JsValue| {
                        let result = fr2.result().unwrap();
                        let arr = Uint8Array::new(&result);
                        let mut vec = vec![0u8; arr.length() as usize];
                        arr.copy_to(&mut vec[..]);
                        *SELECTED_IMAGE.lock().unwrap() = Some((vec, name));
                    }) as Box<dyn FnOnce(_)>);
                    fr.set_onload(Some(onload.as_ref().unchecked_ref()));
                    onload.forget();
                    let _ = fr.read_as_array_buffer(&file);
                }
            }
        }) as Box<dyn FnMut(_)>);

        input.set_onchange(Some(onchange.as_ref().unchecked_ref()));
        onchange.forget(); // keep alive

        // Trigger the native file dialog
        let _ = input.click();
    }

    pub fn take_selected_image_bytes() -> Option<(Vec<u8>, String)> {
        SELECTED_IMAGE.lock().unwrap().take()
    }

    /// Trigger an async fetch of a bundled asset (relative URL). The bytes+filename will be stored
    /// in the same internal buffer and returned later from `take_selected_image_bytes()`.
    pub fn request_asset(path: &str) {
        let path = path.to_string();
        let window = match web_sys::window() { Some(w) => w, None => return };
        let promise = window.fetch_with_str(&path);
        // Convert to future and read array buffer
        wasm_bindgen_futures::spawn_local(async move {
            match wasm_bindgen_futures::JsFuture::from(promise).await {
                Ok(resp_val) => {
                    let resp: web_sys::Response = resp_val.dyn_into().unwrap();
                    match resp.array_buffer() {
                        Ok(promise) => {
                            match wasm_bindgen_futures::JsFuture::from(promise).await {
                                Ok(buf_val) => {
                                    let arr = Uint8Array::new(&buf_val);
                                    let mut vec = vec![0u8; arr.length() as usize];
                                    arr.copy_to(&mut vec[..]);
                                    let filename = std::path::Path::new(&path).file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or(path.clone());
                                    *SELECTED_IMAGE.lock().unwrap() = Some((vec, filename));
                                }
                                Err(e) => {
                                    web_sys::console::error_1(&e);
                                }
                            }
                        }
                        Err(e) => {
                            web_sys::console::error_1(&e);
                        }
                    }
                }
                Err(e) => {
                    web_sys::console::error_1(&e);
                }
            }
        });
    }
}

#[cfg(target_arch = "wasm32")]
pub use web::{open_image_picker, take_selected_image_bytes, request_asset};

#[cfg(not(target_arch = "wasm32"))]
// Native stubs; native builds use rfd::FileDialog directly
pub fn open_image_picker() {}

#[cfg(not(target_arch = "wasm32"))]
pub fn take_selected_image_bytes() -> Option<(Vec<u8>, String)> { None }

#[cfg(not(target_arch = "wasm32"))]
/// Request loading an asset. On native this performs a synchronous file read and returns bytes+filename.
/// On wasm this function is a no-op (the wasm version triggers an async fetch and returns None immediately).
pub fn request_asset(path: &str) -> Option<(Vec<u8>, String)> {
    match std::fs::read(path) {
        Ok(bytes) => Some((bytes, std::path::Path::new(path).file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| path.to_owned()))),
        Err(_) => None,
    }
}
