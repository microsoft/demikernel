# C# / .NET bindings

To build:

First, install the dotnet SDK (at least v7.0): https://dotnet.microsoft.com/en-us/download/dotnet (see
  also [ubuntu 22.10](https://learn.microsoft.com/en-us/dotnet/core/install/linux-ubuntu#2210) and [ubuntu 22.04](https://learn.microsoft.com/en-us/dotnet/core/install/linux-ubuntu#2204))

If you are using an IDE, you can load the `Demikernel.sln` file to build. If you are using the command-line, in this directory;

To build the main library (in debug mode):

``` txt
dotnet build Build.csproj
```

To package the main library for deployment (note that this requires the native binaries to have already been built, for example `libdemikernel.so`):

``` txt
dotnet pack -c Release Build.csproj
```

The package is currently listed on nuget.org as [Demikernel](https://www.nuget.org/packages/Demikernel/)

# Testing

To run the tests, you need the LibOs environment variables; for example:

```
export LIBOS=Catnap
export CONFIG_PATH=$HOME/config.yaml
export RUST_LOG=debug
```

then:

``` txt
dotnet test Test.csproj
```

(or use your IDE's test runner)


# Notes

The raw Demikernel API is exposed via `Demikernel.Interop.Libdemiknel`; however, in general-purpose applications it is *not*
suggested to use this as the primary API; the API is exposed in a more .NET-focused way via `Socket` *noting that this
is not optimal for Demikernel* (as it is not a tight loop); for example:

``` c#
using var socket = Socket.Create(AddressFamily.InterNetwork, SocketType.Stream, ProtocolType.Tcp);
socket.Bind(new IPEndPoint(IPAddress.Loopback, 4133));
socket.Listen(32);
//...
var accept = await socket.AcceptAsync(ct.Token);
using var client = accept.AsSocket();
```