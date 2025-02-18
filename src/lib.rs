use ngx::ffi::{
    nginx_version, ngx_array_push, ngx_chain_t, ngx_command_t, ngx_conf_log_error, ngx_conf_t, ngx_buf_t, ngx_http_core_module,
    ngx_http_handler_pt, ngx_http_module_t, ngx_http_phases_NGX_HTTP_CONTENT_PHASE,
    ngx_int_t, ngx_module_t, ngx_uint_t, NGX_CONF_NOARGS,
    NGX_HTTP_LOC_CONF, NGX_HTTP_MODULE, NGX_LOG_ERR, NGX_HTTP_LOC_CONF_OFFSET, NGX_RS_MODULE_SIGNATURE
};
use ngx::http::{ HTTPModule, MergeConfigError, Method, HTTPStatus, ngx_http_conf_get_module_loc_conf };
use ngx::{ core, core::Buffer, http };
use ngx::{ http_request_handler, ngx_log_debug_http, ngx_modules, ngx_string, };

use std::os::raw::{ c_char, c_void };

use std::ptr::{ addr_of, copy };

use ::http::Uri;

use std::cmp::max;
use std::collections::HashMap;
use std::ffi::CString;
use std::fs::read_to_string;
use std::io::Write;
use std::path::PathBuf;
use std::sync::RwLock;
use std::time::Instant;

use serde::{ Serialize };

use once_cell::sync::Lazy;

use uuid::Uuid;

use urlencoding;

mod localhost;

use localhost::is_localhost;

mod photos;

pub use photos::MD_FILE;
pub use photos::Image;
pub use photos::make_preview;

use photos::as_preview;
use photos::as_scaled;
use photos::is_jpg;
use photos::load_file;
use photos::load_metadata;
use photos::is_mp4;
use photos::resize_image;
use photos::update_caption;

struct Module;

// Store the metadata in RAM so we don't have to reparse everything on each request.
static IMAGES: Lazy<RwLock<HashMap<String, Vec<Image>>>> = Lazy::new(|| RwLock::new(HashMap::<String, Vec<Image>>::new()));

// Most of the boilerplate nginx code uses https://github.com/f5yacobucci/ngx-rust-howto as an example.

impl http::HTTPModule for Module {
    type MainConf = ();
    type SrvConf = ();
    type LocConf = ModuleConfig;

    unsafe extern "C" fn postconfiguration(cf: *mut ngx_conf_t) -> ngx_int_t {
        let htcf = http::ngx_http_conf_get_module_main_conf(cf, &*addr_of!(ngx_http_core_module));

        let h = ngx_array_push(
            &mut (*htcf).phases[ngx_http_phases_NGX_HTTP_CONTENT_PHASE as usize].handlers,
        ) as *mut ngx_http_handler_pt;
        if h.is_null() {
            return core::Status::NGX_ERROR.into();
        }

        // set an Access phase handler
        *h = Some(rust_gallery_access_handler);
        core::Status::NGX_OK.into()
    }
}

// Create a ModuleConfig to save our configuration state.
#[derive(Debug, Default)]
struct ModuleConfig {
    enabled: bool,
    root: String           // root path for files to be served
}

impl http::Merge for ModuleConfig {
    fn merge(&mut self, prev: &ModuleConfig) -> Result<(), MergeConfigError> {
        if prev.enabled {
            self.enabled = true;
        }

        if self.root.is_empty() {
            self.root = String::from(if !prev.root.is_empty() {
                &prev.root
            } else {
                ""
            });
        }

        if self.enabled && self.root.is_empty() {
            return Err(MergeConfigError::NoValue);
        }

        Ok(())
    }
}

// Create our "C" module context with function entrypoints for NGINX event loop. This "binds" our
// HTTPModule implementation to functions callable from C.
#[no_mangle]
static ngx_http_rust_gallery_module_ctx: ngx_http_module_t = ngx_http_module_t {
    preconfiguration: Some(Module::preconfiguration),
    postconfiguration: Some(Module::postconfiguration),
    create_main_conf: Some(Module::create_main_conf),
    init_main_conf: Some(Module::init_main_conf),
    create_srv_conf: Some(Module::create_srv_conf),
    merge_srv_conf: Some(Module::merge_srv_conf),
    create_loc_conf: Some(Module::create_loc_conf),
    merge_loc_conf: Some(Module::merge_loc_conf),
};

// Create our module structure and export it with the `ngx_modules!` macro. For this simple
// handler, the ngx_module_t is predominately boilerplate save for setting the above context into
// this structure and setting our custom configuration command (defined below).
ngx_modules!(ngx_http_rust_gallery_module);

#[no_mangle]
pub static mut ngx_http_rust_gallery_module: ngx_module_t = ngx_module_t {
    ctx_index: ngx_uint_t::max_value(),
    index: ngx_uint_t::max_value(),
    name: std::ptr::null_mut(),
    spare0: 0,
    spare1: 0,
    version: nginx_version as ngx_uint_t,
    signature: NGX_RS_MODULE_SIGNATURE.as_ptr() as *const c_char,

    ctx: &ngx_http_rust_gallery_module_ctx as *const _ as *mut _,
    commands: unsafe { &ngx_http_rust_gallery_commands[0] as *const _ as *mut _ },
    type_: NGX_HTTP_MODULE as ngx_uint_t,

    init_master: None,
    init_module: None,
    init_process: None,
    init_thread: None,
    exit_thread: None,
    exit_process: None,
    exit_master: None,

    spare_hook0: 0,
    spare_hook1: 0,
    spare_hook2: 0,
    spare_hook3: 0,
    spare_hook4: 0,
    spare_hook5: 0,
    spare_hook6: 0,
    spare_hook7: 0,
};

// Register and allocate our command structures for directive generation and eventual storage.
#[no_mangle]
static mut ngx_http_rust_gallery_commands: [ngx_command_t; 2] = [
    ngx_command_t {
        name: ngx_string!("rust_gallery"),
        type_: (NGX_HTTP_LOC_CONF | NGX_CONF_NOARGS) as ngx_uint_t,
        set: Some(ngx_http_rust_gallery_commands_set_method),
        conf: NGX_HTTP_LOC_CONF_OFFSET,
        offset: 0,
        post: std::ptr::null_mut(),
    },
    ngx_command_t::empty(),
];

#[no_mangle]
extern "C" fn ngx_http_rust_gallery_commands_set_method(
    cf: *mut ngx_conf_t,
    _cmd: *mut ngx_command_t,
    conf: *mut c_void,
) -> *mut c_char {
    unsafe {
        let lc = ngx_http_conf_get_module_loc_conf(cf, &*addr_of!(ngx_http_core_module));

        let conf = &mut *(conf as *mut ModuleConfig);
        conf.enabled = true;
        if (*lc).root.data.is_null() {
            let err = CString::new(format!("No root directive for location {}", (*lc).name)).unwrap();
            ngx_conf_log_error(NGX_LOG_ERR as usize, cf, 0, err.as_ptr() as *const c_char);
        }
        conf.root = (*lc).root.to_string();
    };

    std::ptr::null_mut()
}
// End of nginx boilerplate

fn return_value(request: &mut http::Request, s: &str, content_type: &str) -> core::Status
{
    return_value_with_status(request, s, content_type, HTTPStatus::OK)
}

fn return_value_with_status(request: &mut http::Request, s: &str, content_type: &str, status: HTTPStatus) -> core::Status
{
    let mut buffer = request.pool().create_buffer_from_str(&s).unwrap();
    buffer.set_last_buf(true);

    let mut out = ngx_chain_t { buf: buffer.as_ngx_buf_mut(), next: std::ptr::null_mut() };

    request.set_status(status);
    request.add_header_out("Content-Type", content_type);
    request.send_header();
    request.output_filter(&mut out)
}

fn parse_query_string(query: &str) -> HashMap<String, String> {
    query
        .split('&')
        .filter_map(|s| {
            s.split_once('=')
                .and_then(|t| Some((t.0.to_owned(), t.1.to_owned())))
        })
        .collect()
}

#[derive(Serialize,Debug)]
struct Metadata<'a> {
    pub date: String,
    pub caption: &'a String,
    pub video: bool,
    pub location: &'a Option<String>
}

// Metadata used by the web-page
fn return_metadata(request: &mut http::Request, gallery_path: &String) -> core::Status {
    let map = IMAGES.read().unwrap();
    let imgs = map.get(gallery_path).expect("metadata doesn't exist");
    let mut metadata = Vec::<Metadata>::with_capacity(imgs.len());
    for img in imgs.iter() {
        metadata.push(Metadata { 
            date: img.time.format("%m/%d/%Y").to_string(), 
            caption: &img.caption, 
            video: img.is_mp4(),
            location: &img.location
        });
    }

    return_value(request, format!("const metadata = {};", serde_json::to_string(&metadata).unwrap()).as_str(), "application/javascript")
}

// Return '12' from '12.jpg' (for example)
fn get_id(file_name: &str) -> Result<usize, core::Status> {
    let id = &file_name[0..file_name.find(".").expect("Found a period")];
    for c in id.chars() {
        if !c.is_numeric() {
            return Err(core::Status::NGX_DECLINED);
        }
    }
    return Ok(id.parse::<usize>().expect("Failure parsing an id") - 1);
}

#[derive(PartialEq)]
enum FileType {
    JPG,
    MP4
}

fn get_filename_from_id(gallery_path: &String, id: usize, file_type: FileType) -> String {
    let map = IMAGES.read().unwrap();
    let images = map.get(gallery_path).expect("metadata exists");
    let image = images.get(id).expect("Image index in range");
    if image.is_mp4() {
        if file_type == FileType::MP4 {
            if image.mp4_scaled {
                return as_scaled(&image.path);
            } else {
                return format!("{}", image.path);
            }
        } else {
            return as_preview(&image.path);
        }
    }
    return format!("{}", image.path);
}

fn get_file_path(gallery_path: &String, id: usize, file_type: FileType) -> PathBuf {
    let mut file_path = PathBuf::from(gallery_path);
    let file_name = get_filename_from_id(gallery_path, id, file_type);
    file_path.push(file_name.as_str());

    file_path
}

// Used to avoid memory copies; copy directly into nginx buffers
pub struct NginxBuffer<'a> {
    request: &'a mut http::Request,
    first_chain: *mut ngx_chain_t,
    last_chain: *mut ngx_chain_t
}

static MIN_BUF_SIZE: usize = 0x10000;  // 64K

unsafe fn can_write(buf: *const ngx_buf_t, len: usize) -> bool {
    return (*buf).end.offset_from((*buf).last) >= len as isize; 
}

impl Write for NginxBuffer<'_> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let needs_allocation = unsafe {
            self.last_chain.is_null() || !can_write((*self.last_chain).buf, buf.len())
        };
        if needs_allocation {
            let new_chain = self.request.pool().calloc_type::<ngx_chain_t>();

            let buf_size = max(MIN_BUF_SIZE, buf.len());
            let mut buffer = self.request.pool().create_buffer(buf_size).expect("Unable to create buffer");
            
            unsafe {
                (*new_chain).buf = buffer.as_ngx_buf_mut();
                (*new_chain).next = std::ptr::null_mut();

                if self.last_chain.is_null() {
                    self.first_chain = new_chain;
                } else {
                    (*self.last_chain).next = new_chain;
                }
            }
            self.last_chain = new_chain;
        }

        unsafe {
            let ngx_buf_mut = (*self.last_chain).buf;
            copy(buf.as_ptr(), (*ngx_buf_mut).last, buf.len());
            (*ngx_buf_mut).last = (*ngx_buf_mut).last.offset(buf.len() as isize);
        }

        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn get_content_type(file_name: &str) -> &str {
    if is_jpg(&String::from(file_name)) {
        return "image/jpeg";
    }
    if file_name.ends_with(".html") {
        return "text/html";
    }
    ""
}

// Get CSRF crumb for caption edit
fn get_crumb(request: &http::Request) -> &str {
    let mut i = request.headers_in_iterator();
    loop {
        match i.next() {
            Some(h) => {
                if h.0 == "Cookie" {
                    if h.1.starts_with("crumb=") {
                        let (_, crumb) = h.1.split_at("crumb=".len());
                        return crumb;
                    }
                }
            },
            None => {
                return "";
            }
        }
    }
}

fn return_raw_file(request: &mut http::Request, file_name: &str, gallery_path: &String) -> core::Status {
    let mut buffer = NginxBuffer {
        request: request,
        first_chain: std::ptr::null_mut(),
        last_chain: std::ptr::null_mut()
    };
    let mut path = PathBuf::from(gallery_path);
    path.push(file_name);
    load_file(path.as_path(), &mut buffer); 

    respond(&mut buffer, get_content_type(file_name))
}

fn respond(buffer: &mut NginxBuffer, content_type: &str) -> core::Status {
    unsafe {
        (*(*buffer.last_chain).buf).set_last_buf(1);
    }

    buffer.request.set_status(HTTPStatus::OK);
    buffer.request.add_header_out("Content-Type", content_type);
    
    let crumb = get_crumb(buffer.request);
    if crumb.is_empty() {
        buffer.request.add_header_out("Set-Cookie", format!("crumb={}; HttpOnly", Uuid::new_v4()).as_str());
    }

    buffer.request.send_header();

    unsafe {
        buffer.request.output_filter(&mut (*buffer.first_chain))
    }
}

// Resizes the jpg to fit the screen
fn return_jpg(request: &mut http::Request, query_string: Option<&str>, file_name: &str, uri_path: &str, gallery_path: &String) -> core::Status {
    let start = Instant::now();

    let photo_id = match get_id(&file_name) {
        Ok(id) => id,
        Err(_) => { return core::Status::NGX_DECLINED; }
    };

    let query = parse_query_string(
        match query_string {
            Some(qs) => qs,
            None => {
                // Return the full size image if there's no size parameters to resize to.
                let raw_jpg = get_filename_from_id(&gallery_path, photo_id, FileType::JPG);

                return request.internal_redirect(get_raw_uri(uri_path, &raw_jpg).as_str());
            }            
        });

    let file_path = get_file_path(&gallery_path, photo_id, FileType::JPG);
    
    let mut buffer = NginxBuffer {
        request: request,
        first_chain: std::ptr::null_mut(),
        last_chain: std::ptr::null_mut()
    };
    
    resize_image(file_path.as_path(), 
                    query.get("w").expect("No width in uri").parse::<u32>().expect("Bad image width"),
                    query.get("h").expect("No height in uri").parse::<u32>().expect("Bad image height"),
                    &mut buffer);

    let result = respond(&mut buffer, "image/jpeg");
    ngx_log_debug_http!(request, "rust gallery image resize duration: {:?}", start.elapsed());

    return result;
}

// Get uri to the raw file, not the 'id' file
fn get_raw_uri(uri_path: &str, file_name: &String) -> String {
    let mut uri = uri_path.to_string();
    uri.push_str("/");
    uri.push_str(file_name.as_str());

    uri
}

fn return_mp4(request: &mut http::Request, file_name: &str, uri_path: &str, gallery_path: &String) -> core::Status {
    if file_name.ends_with(".scaled.mp4") {
        return core::Status::NGX_DECLINED;
    }

    let video_id = match get_id(&file_name) {
        Ok(id) => id,
        Err(_) => { return core::Status::NGX_DECLINED; }        
    };


    let mp4name = get_filename_from_id(&gallery_path, video_id, FileType::MP4);

    return request.internal_redirect(get_raw_uri(uri_path, &mp4name).as_str());
}

fn get_metadata_file(path: &String) -> PathBuf {
    let mut r = PathBuf::from(&path);
    r.push(MD_FILE);

    return r;
}

// Return 'edit_caption.js if the client is 'localhost'.
fn return_edit_caption(request: &mut http::Request, gallery_path: &String) -> core::Status {
    let rv = if is_localhost(request) {
        let mut js_path = PathBuf::from(gallery_path);
        js_path.push("edit_caption.js");
    
        let js = read_to_string(js_path.as_path()).expect("edit_caption.js doesn't exist");
        format!("const crumb = \"{}\";\n\n{}", get_crumb(request), js)
    } else {
        String::from("{}")
    };
    return_value(request, rv.as_str(), "application/javascript")
}

fn handle_caption(request: &mut http::Request, query_string: Option<&str>, id_str: &str, gallery_path: &String) -> core::Status {
    if !is_localhost(request) {
        eprintln!("Attempt to edit a caption without being localhost");
        return return_value_with_status(request, "Not permitted", "text/plain", HTTPStatus::UNAUTHORIZED);
    }
    let id = id_str.parse::<usize>().expect("Failure parsing an id") - 1;

    let query = parse_query_string(query_string.expect("Badly formed query string"));
    let crumb = match query.get("crumb") {
        Some(c) => c,
        None => {
            eprintln!("Attempt to edit a caption with no crumb. CSRF attack?");
            return return_value_with_status(request, "Not permitted", "text/plain", HTTPStatus::UNAUTHORIZED);
        }
    };
    if *crumb != get_crumb(request) {
        eprintln!("Attempt to edit a caption without the correct crumb. CSRF attack?");
        return return_value_with_status(request, "Not permitted", "text/plain", HTTPStatus::UNAUTHORIZED);
    }
    let caption = match query.get("caption") {
        Some(c) => c,
        None => {
            return return_value_with_status(request, "Not permitted", "text/plain", HTTPStatus::UNAUTHORIZED);
        }
    };

    let mut rv = String::from("Ok");
    let mut status = HTTPStatus::OK;
    match update_caption(get_metadata_file(gallery_path).as_path(), id, &format!("{}", urlencoding::decode(caption).unwrap())) {
        Ok(_) => (),
        Err(e) => {
            rv = format!("Failed to write with error: {}", e.to_string());
            status = HTTPStatus::UNAUTHORIZED;
        }
    }

    // Will cause a lazy load of metadata on the next request, to pick up the new caption
    (*IMAGES.write().unwrap()).remove(gallery_path);

    // Need to respond with something.
    return_value_with_status(request, rv.as_str(), "text/plain", status)
}

// Implement a request handler. The convenience macro (http_request_handler!) will
// convert the native NGINX request into a Rust Request instance as well as define an extern C
// function callable from NGINX.
http_request_handler!(rust_gallery_access_handler, |request: &mut http::Request| {
    let enabled: bool;
    let root_path: String;
    {
        let co = unsafe { request.get_module_loc_conf::<ModuleConfig>(&*addr_of!(ngx_http_rust_gallery_module)) }.expect("Module config exists");
        enabled = co.enabled;
        root_path = co.root.clone();
    }

    if !enabled {
        return core::Status::NGX_DECLINED;
    }

    request.discard_request_body();

    let uri = request.unparsed_uri().to_str().expect("Uri not UTF8").parse::<Uri>().expect("Unable to parse uri");
    let uri_path = String::from(request.path().to_str().expect("Path not UTF8"));
    let query_string = uri.query();
    let (uri_path, file_name) = match uri_path.rsplit_once('/') {
        Some(x) => x,
        None => { return core::Status::NGX_DECLINED }
    };

    let gallery_path = format!("{}{}", root_path, uri_path); 

    if IMAGES.read().unwrap().get(&gallery_path).is_none() {
        let r = get_metadata_file(&gallery_path);
        let md = match load_metadata(r.as_path()) {
            Ok(m) => m,
            Err(_) => {
                // Should really send the 404 page configured for the nginx location
                return return_value_with_status(request, "404 Not found", "text/html", HTTPStatus::NOT_FOUND);
            }
        };
        let mut map = IMAGES.write().unwrap();
        (*map).insert(gallery_path.clone(), md);
    }

    ngx_log_debug_http!(request, "Rust Gallery handling: {}", file_name);

    match file_name {
        "metadata"        => return_metadata(request, &gallery_path),
        "thumbnails.jpg"  => return_raw_file(request, file_name, &gallery_path),
        "edit_caption.js" => return_edit_caption(request, &gallery_path),
        _ => {
            let f_n = &file_name.to_string(); 
            if is_jpg(f_n) {
                return return_jpg(request, query_string, file_name, uri_path, &gallery_path);
            }
            if is_mp4(f_n) {
                return return_mp4(request, file_name, uri_path, &gallery_path);
            }
            if request.method() == Method::POST {
                return handle_caption(request, query_string, file_name, &gallery_path);
            }
            return_raw_file(request, "index.html", &gallery_path)
        }
    }
});
