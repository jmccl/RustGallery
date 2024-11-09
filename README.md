This project enables self-hosting of photo galleries. It is immature and not
full-featured. I wrote it to host my vacation photos for friends and family.

It has a few things in its favor.

* It resizes photos on the fly to match the display size available on the client browser. This
results in trading off increased server CPU for minimizing bandwidth and storage on the server, while displaying the largest image that the client can consume. This is likely a reasonable trade-off
for low-volume serving to friends and family and a terrible trade-off for high-volume
serving.
* It has - somewhat limited - support for videos.
* It has support for captions for photos and videos.
* Mobile support is not great, but it exists.

I wrote this mainly to teach myself the Rust programming language. There are
many self-hosted photo gallery projects and this one is unlikely
to be the one that's best for you. Some of the issues are:

* It requires the use of nginx as the web server.
* It is based on the _[ngx-rust](https://github.com/nginxinc/ngx-rust)_ crate. Its
README currently starts with: "This project is still a work in progress and not production ready."
* Photo resizing on the fly is somewhat slow if the originals are very large, although caching tactics in the browser mitigate this. In addition resizing is not done asynchronously, i.e., it blocks an nginx thread.
* There is essentially no custimization possible (at present?)
* At least minor fixes will likely be needed if not hosting on Linux.
* As this is my my first Rust code, it's likely not idiomatic. As this is my first nginx module, it's likely not idiomatic.

You can view an example gallery [here](https://squotd.net/RustGalleryDemo/). (Note that [photo 36](https://squotd.net/RustGalleryDemo/#36) demonstrates how videos are displayed.)

## Building

Assuming a local rust and nginx install, run this:

>$> ./build

Alternativley, there's a docker build for debian. Run this from the _docker_ directory:

>$> docker build .

## Gallery Preparation

Prior to serving a gallery some pre-processing must occur to gather metadata
and create previews and downsampled files for the videos, if necessary.

The files to be shown in the gallery (with extension 'jpg', 'mp4', 'mov' or 'avi') should all be
in one directory. The executable _make-gallery_ should be run from the directory
with the files. This will write a number of files into the gallery to allow it
to be served.

Directory preparation requires _exiftool_ to be installed. If there are any videos
then _ffmpeg_ needs to be installed. Note that neither of these need to be 
installed on the server running nginx that will serve the gallery. These 
are only required for gallery preparation.

### Captions

If nginx is serving from localhost (127.0.0.1) captions may be edited by double-clicking
on the picture id in the upper left of the web page. (Note that the generated _metadata_ file must
be writable by the nginx child processes user, which varies by OS.)

Alternatively - and possibly more conveniently when migrating from another gallery -
captions may be placed in a text file named _captions.txt_, one entry 
per line with the source file name followed by the caption. For example:

```
foo.jpg This is the caption
bar.jpg This is another caption
```

The file must be in the gallery directory when _make-gallery_ is run.

(In the unlikely event that you're migrating from PyGallery you can
extract the existing captions with this [script](PyGalleryConversion/extract_captions).)

## Nginx Configuration

Your nginx configuration should look something like this.

```
load_module <path>/librust_gallery.so;

...

http {

    ...

    server {

        ...

        location /gallery {
            root <path>;
            rust_gallery;

        ...

        }
    }
}
```

The _root_ directive must exist; it will not be picked up from parent directives.
The _root_ directive must also precede the _rust_gallery_ directive.

## Motivation

For decades I have self-hosted vacation photos with [PyGallery](https://pygallery.sourceforge.net/), unsupported since 2003. 
At long last I have concluded that continueing this is somewhat silly. I wrote my own code
rather than using one of the many (likely better) alternatives because:

* I wanted a project to try out Rust.
* I had been a part of discussions as to whether to use nginx in an enterprise setting
 in the past and wanted to write an nginx module to see how it worked.
* I wanted to minimize storage costs while still keeping the full size original photos.
* I wanted to host the photos on S3. Although this did not end up being 
part of this project, I run it this way using [mountpoint-s3](https://github.com/awslabs/mountpoint-s3). I document how I do that [here](mountpoint-s3/README.md).
