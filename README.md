# Test suit to test MCTP over I2C through a chardev exposed by QEMU

The tools use Unix-Sockets for communication.

There are currently two binary targets, `echo` and `initiator`.

The `initiator` tries to connect to a socket and then opens a request and waits for a response.

`echo` opens a new socket (server mode) by default and listens for an incoming stream.
As soon as a new stream is opened, it waits for a request and echos the the payload as response.

## Defaults
Both targets have a set of compile-time defaults, some of them can be overwritten with environment variables at runtime.
See the source files (`src/bin/initiator.rs` & `src/bin/echo.rs`) for more.

