using Demikernel.Interop;
using System.Buffers;
using System.Net.Sockets;
using System.Net;
using System.Runtime.CompilerServices;
using System.Text;
using Demikernel.Internal;

namespace Demikernel;

/// <summary>
/// Represents a network device driven by the demikernel API; this is a simplified
/// and generalised API that is broadly comparable to the <see cref="System.Net.Sockets.Socket"/>,
/// however, it is not directly optimized as a DemiKernel message pump.
/// </summary>
public readonly struct Socket : IDisposable
{
    private readonly int _qd;
    // ~DKSocket() => ReleaseHandle(); // uncomment if we go for class

#if SYNC_WAIT && SYNC_WAIT_COUNT
    static int s_SyncPush, s_SyncPop, s_SyncAccept;
    static async Task WriteCounters()
    {
        while (true)
        {
            await Task.Delay(5000);
            Debug.WriteLine($"sync push: {Volatile.Read(ref s_SyncPush)}, pop: {Volatile.Read(ref s_SyncPop)}, accept: {Volatile.Read(ref s_SyncAccept)}");
        }
    }
    static DKSocket() => Task.Run(WriteCounters);
#endif

    /// <summary>
    /// Initialize the API
    /// </summary>
    public static unsafe void Initialize(params string[] args)
    {
        args ??= Array.Empty<string>();
        int len = args.Length; // for the NUL terminators
        if (args.Length > 128) throw new ArgumentOutOfRangeException(nameof(args), "Too many args, sorry");
        foreach (var arg in args)
        {
            len += Encoding.ASCII.GetByteCount(arg);
        }
        var lease = ArrayPool<byte>.Shared.Rent(len);
        fixed (byte* raw = lease)
        {
            byte** argv = stackalloc byte*[args.Length];
            int byteOffset = 0;
            for (int i = 0; i < args.Length; i++)
            {
                argv[i] = &raw[byteOffset];
                var arg = args[i];
                var count = Encoding.ASCII.GetBytes(arg, 0, arg.Length, lease, byteOffset);
                byteOffset += count;
                lease[byteOffset++] = 0; // NUL terminator
            }
            ApiResult result;
            lock (LibDemikernel.GlobalLock)
            {
                result = LibDemikernel.init(args.Length, argv);
            }
            result.AssertSuccess(nameof(LibDemikernel.init));
        }
        ArrayPool<byte>.Shared.Return(lease);
    }

    /// <summary>
    /// Create a new socket
    /// </summary>
    public static unsafe Socket Create(AddressFamily addressFamily, SocketType socketType, ProtocolType protocolType)
    {
        int qd = 0;
        ApiResult result;
        lock (LibDemikernel.GlobalLock)
        {
            result = LibDemikernel.socket(&qd, addressFamily, socketType, protocolType);
        }
        result.AssertSuccess(nameof(LibDemikernel.socket));
        return new Socket(qd);
    }
    internal Socket(int qd)
    {
        _qd = qd;
    }
    /// <summary>
    /// Close the socket
    /// </summary>
    public void Close() => Dispose();

    /// <summary>
    /// Release resources associated with this instance
    /// </summary>
    public void Dispose()
    {
        GC.SuppressFinalize(this);
        Free();
    }

    private void Free()
    {
        // do our best to prevent double-release, noting that this isn't perfect (think: struct copies)
        var qd = Interlocked.Exchange(ref Unsafe.AsRef(in _qd), 0);
        if (qd != 0)
        {
            ApiResult result;
            lock (LibDemikernel.GlobalLock)
            {
                result = LibDemikernel.close(qd);
            }
            result.AssertSuccess(nameof(LibDemikernel.close));
        }
    }

    /// <inheritdoc/>
    public override string ToString() => $"LibOS socket {_qd}";

    /// <summary>
    /// As a server socket, prepare to listen for incoming connections
    /// </summary>
    public void Listen(int backlog)
    {
        ApiResult result;
        lock (LibDemikernel.GlobalLock)
        {
            result = LibDemikernel.listen(_qd, backlog);
        }
        result.AssertSuccess(nameof(LibDemikernel.listen));
    }

    /// <summary>
    /// Bind the socket to a specific endpoint
    /// </summary>
    public unsafe void Bind(EndPoint endpoint)
    {
        var socketAddress = endpoint.Serialize();
        var len = socketAddress.Size;
        if (len > 128) throw new ArgumentException($"Socket address is oversized: {len} bytes");

        var saddr = stackalloc byte[len];
        for (int i = 0; i < len; i++)
        {
            saddr[i] = socketAddress[i];
        }
        ApiResult result;
        lock (LibDemikernel.GlobalLock)
        {
            result = LibDemikernel.bind(_qd, saddr, len);
        }
        result.AssertSuccess(nameof(LibDemikernel.bind));
    }

    /// <summary>
    /// As a server socket, accept an inbound connection
    /// </summary>
    public unsafe ValueTask<AcceptResult> AcceptAsync(CancellationToken cancellationToken = default)
    {
        Unsafe.SkipInit(out QueueToken qt);
        ApiResult result;
#if SYNC_WAIT
        int syncResult = -1;
        Unsafe.SkipInit(out QueueResult qr);
#endif
        lock (LibDemikernel.GlobalLock)
        {
            result = LibDemikernel.accept(&qt.qt, _qd);
#if SYNC_WAIT
            if (result == 0)
            {
                fixed (TimeSpec* ptr = &TimeSpec.Zero)
                {
                    syncResult = LibDemikernel.wait(&qr, qt.qt, ptr);
                }
            }
#endif
        }
        result.AssertSuccess(nameof(LibDemikernel.accept));
#if SYNC_WAIT
        if (syncResult == 0)
        {
#if SYNC_WAIT_COUNT
            Interlocked.Increment(ref s_SyncAccept);
#endif
            qr.Assert(Opcode.Accept);
            return new(qr.ares);
        }
#endif
        return new(PendingQueue.AddAccept(qt, cancellationToken));
    }

    /// <summary>
    /// As a server socket, accept an inbound connection
    /// </summary>
    public unsafe AcceptResult Accept()
    {
        var pending = AcceptAsync();
        if (pending.IsCompleted) return pending.Result;
        return pending.AsTask().Result;
    }

    /// <summary>
    /// Read data from the remote device
    /// </summary>
    public unsafe ValueTask<ScatterGatherArray> ReceiveAsync(CancellationToken cancellationToken = default)
    {
        Unsafe.SkipInit(out QueueToken qt);
        ApiResult result;
#if SYNC_WAIT
        int syncResult = -1;
        Unsafe.SkipInit(out QueueResult qr);
#endif
        lock (LibDemikernel.GlobalLock)
        {
            result = LibDemikernel.pop(&qt.qt, _qd);
#if SYNC_WAIT
            if (result == 0)
            {
                fixed (TimeSpec* ptr = &TimeSpec.Zero)
                {
                    syncResult = LibDemikernel.wait(&qr, qt.qt, ptr);
                }
            }
#endif
        }
        result.AssertSuccess(nameof(LibDemikernel.pop));
#if SYNC_WAIT
        if (syncResult == 0)
        {
#if SYNC_WAIT_COUNT
            Interlocked.Increment(ref s_SyncPop);
#endif
            qr.Assert(Opcode.Pop);
            return new(qr.sga);
        }
#endif
        return new(PendingQueue.AddReceive(qt, cancellationToken));
    }

    /// <summary>
    /// Read data from the remote device
    /// </summary>
    public unsafe ScatterGatherArray Receive()
    {
        var pending = ReceiveAsync();
        if (pending.IsCompleted) return pending.Result;
        return pending.AsTask().Result;
    }

    /// <summary>
    /// Send data to the remote device
    /// </summary>
    public unsafe Task SendAsync(in ScatterGatherArray payload, CancellationToken cancellationToken = default)
    {
        if (payload.IsEmpty) return Task.CompletedTask;
        Unsafe.SkipInit(out QueueToken qt);
        ApiResult result;
#if SYNC_WAIT
        int syncResult = -1;
        Unsafe.SkipInit(out QueueResult qr);
#endif
        fixed (ScatterGatherArray* ptr = &payload)
        {
            lock (LibDemikernel.GlobalLock)
            {
                result = LibDemikernel.push(&qt.qt, _qd, ptr);
#if SYNC_WAIT
                if (result == 0)
                {
                    fixed (TimeSpec* timePtr = &TimeSpec.Zero)
                    {
                        syncResult = LibDemikernel.wait(&qr, qt.qt, timePtr);
                    }
                }
#endif
            }
        }
        result.AssertSuccess(nameof(LibDemikernel.push));
#if SYNC_WAIT
        if (syncResult == 0)
        {
#if SYNC_WAIT_COUNT
            Interlocked.Increment(ref s_SyncPush);
#endif
            qr.Assert(Opcode.Push);
            return default;
        }
#endif
        return PendingQueue.AddVoid(qt, cancellationToken);
    }

    /// <summary>
    /// Send data to the remote device
    /// </summary>
    public void Send(in ScatterGatherArray payload)
    {
        if (payload.IsEmpty) return;
        SendAsync(in payload).Wait();
    }


    /// <summary>
    /// Send data to the remote device
    /// </summary>
    public Task SendAsync(ReadOnlySpan<byte> payload, CancellationToken cancellationToken = default)
    {
        if (payload.IsEmpty) return Task.CompletedTask;
        using var sga = ScatterGatherArray.Create(payload);
        // this looks like we're disposing too soon, but actually it is
        // fine; you can "sgafree" as soon as the "push" has been started
        return SendAsync(in sga, cancellationToken);
    }

    /// <summary>
    /// Send data to the remote device
    /// </summary>
    public void Send(ReadOnlySpan<byte> payload)
    {
        if (payload.IsEmpty) return;
        using var sga = ScatterGatherArray.Create(payload);
        Send(sga);
    }


    /// <summary>
    /// As a client socket, connect to a server
    /// </summary>
    public unsafe Task ConnectAsync(EndPoint endpoint, CancellationToken cancellationToken = default)
    {
        var socketAddress = endpoint.Serialize();
        var len = socketAddress.Size;
        if (len > 128) throw new ArgumentException($"Socket address is oversized: {len} bytes");

        var saddr = stackalloc byte[len];
        for (int i = 0; i < len; i++)
        {
            saddr[i] = socketAddress[i];
        }
        
        ApiResult result;
        Unsafe.SkipInit(out QueueToken qt);
        lock (LibDemikernel.GlobalLock)
        {
            result = LibDemikernel.connect(&qt.qt, _qd, saddr, len);
        }
        result.AssertSuccess(nameof(LibDemikernel.connect));
        return PendingQueue.AddVoid(qt, cancellationToken);
    }

    /// <summary>
    /// As a client socket, connect to a server
    /// </summary>
    public void Connect(EndPoint endpoint)
        => ConnectAsync(endpoint).Wait();
}
