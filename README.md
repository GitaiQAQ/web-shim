# Web-Shim

This project is a URL snapshot generator similar to Urlbox, written in Rust. It utilizes Chrome Headless as the renderer and supports various storage backend options provided by [OpenDAL](https://opendal.apache.org/), including but not limited to S3, local files, and Google Cloud Storage.

## Installation

Follow the steps below to install the service:

1. Download and Extract the Chrome files:
```shell
$ wget https://registry.npmmirror.com/-/binary/chromium-browser-snapshots/Linux_x64/1045489/chrome-linux.zip && unzip chrome-linux.zip
```

2. Make sure Chrome installation is successful:

```shell
$ ./chrome/chrome --version
```

3. Download the appropriate build for your platform from this project's "actions" section or build web-shim yourself.
4. Start the service with the following command:

```shell
$ CHROME=./chrome-linux/chrome ./web-shim
```

Then, visit `https://{YOUR_SERVER_ADDRESS}/screenshot/default/?url=http://example.com` to start using the service.
If everything goes as expected, you should see a screenshot of `http://example.com`.


## Usage
The project works by accepting a URL from the user and generating a snapshot. In addition, it provides the capability to verify request parameters using an AWS Pre Sign-like capability by signing the request parameters with an access token. Upon receiving user requests, these parameters are verified to ensure that the service is not misused.
Another included feature is built-in IP and bucket traffic control for resource management.

## Contributing
To be added.

## License
This project is licensed under the terms of the [MIT](https://opensource.org/licenses/MIT) license.

## References

* [alpine-chrome](https://github.com/Zenika/alpine-chrome)
* [chromiumoxide](https://github.com/mattsse/chromiumoxide)
* [http-rs/tide](https://github.com/http-rs/tide)
