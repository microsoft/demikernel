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
suggested to use this as the primary API; Demikernel is intended to be used as a single-threaded IO core in scenarios where
the primary application functionality is IO related (not CPU offload); this is broadly like `epoll`, but with an emphasis
on the single IO core. The `MessagePump<T>` wraps up the IO loop functionality for most common server scenarios, allowing
a consumer to respond to incoming data. See `EchoServer` for an example of a server that responds to incoming data requests
by pushing the same data back at the sender.