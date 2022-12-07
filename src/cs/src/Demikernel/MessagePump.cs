using Demikernel.Interop;
using System.Buffers;
using System.Diagnostics;
using System.Net;
using System.Net.Sockets;
using System.Numerics;
using System.Runtime.CompilerServices;
using System.Runtime.InteropServices;
using System.Text;

namespace Demikernel;

public abstract class MessagePump : IDisposable, IAsyncDisposable
{
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
            if (Interlocked.CompareExchange(ref s_running, 1, 0) == 0)
            {
                ApiResult result = LibDemikernel.init(args.Length, argv);
                Volatile.Write(ref s_running, 0);
                result.AssertSuccess(nameof(LibDemikernel.init));
            }
        }
        ArrayPool<byte>.Shared.Return(lease);
    }

    internal static int s_running;

    public abstract void Start();
    public abstract void Stop();
    public virtual void Dispose()
    {
        GC.SuppressFinalize(this);
        Stop();
    }

    public ValueTask DisposeAsync()
    {
        GC.SuppressFinalize(this);
        Stop();
        return new(Shutdown);
    }
    private protected readonly TaskCompletionSource _shutdown = new();
    public Task Shutdown => _shutdown.Task;

    [StructLayout(LayoutKind.Explicit, Pack = 1)]
    private protected readonly struct PendingOperation
    {
        [FieldOffset(0)]
        public readonly int Socket;
        [FieldOffset(4)]
        public readonly Opcode Opcode;
        [FieldOffset(8)]
        public readonly ScatterGatherArray Payload; // size: 48

        [FieldOffset(8)]
        public readonly int AddrSize;
        [FieldOffset(12)]
        public readonly byte AddrStart; // max size 44 before starts hitting end

        private const int MaxAddrSize = 44;

        public static PendingOperation Pop(int socket) => new(socket, Opcode.Pop);
        public static PendingOperation Connect(int socket, ReadOnlySpan<byte> address) => new(socket, Opcode.Connect, address);
        public static PendingOperation Push(int socket, in ScatterGatherArray payload) => new(socket, Opcode.Push, in payload);
        public static PendingOperation Accept(int socket) => new(socket, Opcode.Connect);

        internal unsafe long ExecuteDirect()
        {
            long qt = 0;
            switch (Opcode)
            {
                case Opcode.Pop:
                    LibDemikernel.pop(&qt, Socket).AssertSuccess(nameof(LibDemikernel.pop));
                    break;
                case Opcode.Connect:
                    fixed (byte* addrStart = &AddrStart)
                    {
                        LibDemikernel.connect(&qt, Socket, addrStart, AddrSize).AssertSuccess(nameof(LibDemikernel.connect));
                    }
                    break;
                case Opcode.Accept:
                    LibDemikernel.accept(&qt, Socket).AssertSuccess(nameof(LibDemikernel.accept));
                    break;
                case Opcode.Push:
                    fixed (ScatterGatherArray* payload = &Payload)
                    {
                        LibDemikernel.push(&qt, Socket, payload).AssertSuccess(nameof(LibDemikernel.push));
                        LibDemikernel.sgafree(payload).AssertSuccess(nameof(LibDemikernel.sgafree));
                    }
                    break;
                default:
                    ThrowInvalid(Opcode);
                    break;
            }
            return qt;
            static void ThrowInvalid(Opcode opcode) => throw new NotImplementedException("Unexpected operation: " + opcode);
        }

        private unsafe PendingOperation(int socket, Opcode opcode, ReadOnlySpan<byte> address) : this(socket, opcode)
        {
            AddrSize = address.Length;
            fixed (byte* addrStart = &AddrStart)
            {
                address.CopyTo(new Span<byte>(addrStart, MaxAddrSize)); // this max usage intentional to assert overflow conditions
            }
        }

        private PendingOperation(int socket, Opcode opcode, in ScatterGatherArray payload) : this(socket, opcode)
        {
            Payload = payload;
        }

        private PendingOperation(int socket, Opcode opcode)
        {
            Unsafe.SkipInit(out this);
            Socket = socket;
            Opcode = opcode;
        }
    }
}
public abstract class MessagePump<TState> : MessagePump
{
    private readonly HashSet<int> _liveSockets = new();

    public override sealed void Start()
    {
        if (Shutdown.IsCompleted)
        {
            throw new InvalidOperationException("This message-pump has already completed");
        }
        if (Interlocked.CompareExchange(ref s_running, 1, 0) == 0)
        {
            var thd = new Thread(static s => ((MessagePump<TState>)s!).Execute())
            {
                Name = GetType().Name,
                IsBackground = true
            };
            thd.Start(this);
        }
        else
        {
            throw new InvalidOperationException("A message-pump is already running");
        }
    }

    public override sealed void Stop()
    {
        _keepRunning = false;
        lock (_pendingOperations)
        {   // wake up the loop if we're waiting for work
            Monitor.PulseAll(_pendingOperations);
        }
    }

    private readonly Queue<(TState State, PendingOperation Operation)> _pendingOperations = new();
    private volatile bool _pendingWork, _keepRunning = true;
    private int _messagePumpThreadId = -1;

    private long[] _liveOperations = Array.Empty<long>();
    private TState[] _liveStates = Array.Empty<TState>();
    private int _liveOperationCount = 0;

    private unsafe void Execute()
    {
        _messagePumpThreadId = Environment.CurrentManagedThreadId;
        var timeout = TimeSpec.Create(TimeSpan.FromMilliseconds(1)); // entirely arbitrary
        try
        {
            OnStart();
            while (_keepRunning)
            {
                if (_pendingWork)
                {
                    lock (_pendingOperations)
                    {
                        while (_pendingOperations.TryDequeue(out var tuple))
                        {
                            AddAsMessagePump(tuple.State, in tuple.Operation);
                        }
                        _pendingWork = false;
                    }
                }

                if (_liveOperationCount == 0)
                {
                    lock (_pendingOperations)
                    {
                        if (_pendingOperations.Count != 0)
                        {   // already something to do (race condition)
                            _pendingWork = true; // just in case
                        }
                        else
                        {
                            Monitor.Wait(_pendingOperations);
                        }
                    }
                    continue; // go back and add them
                }

                // so, definitely something to do
                Debug.Assert(_liveOperationCount > 0);
                Unsafe.SkipInit(out QueueResult qr);
                int offset = 0;
                ApiResult result;
                fixed (long* qt = _liveOperations)
                {
                    result = LibDemikernel.wait_any(&qr, &offset, qt, _liveOperationCount, &timeout);
                }
                if (result == ApiResult.Timeout) continue;
                result.AssertSuccess(nameof(LibDemikernel.wait_any));
                Debug.Assert(offset >= 0 && offset < _liveOperationCount);

                // fixup the pending tokens by moving the last into the space we're vacating
                var state = _liveStates[offset];
                if (_liveOperationCount > 1)
                {
                    _liveOperations[offset] = _liveOperations[_liveOperationCount - 1];
                    _liveStates[offset] = _liveStates[_liveOperationCount - 1];
                }
                // wipe the state to allow collect if necessary (we don't need to wipe the token)
                _liveStates[--_liveOperationCount] = default!;

                bool beginRead = false;
                int readSocket = qr.Qd;
                TState origState = state;
                switch (qr.Opcode)
                {
                    case Opcode.Accept:
                        beginRead = OnAccept(qr.Qd, ref state, in qr.AcceptResult);
                        readSocket = qr.AcceptResult.Qd;
                        break;
                    case Opcode.Connect:
                        beginRead = OnConnect(qr.Qd, ref state);
                        break;
                    case Opcode.Pop:
                        beginRead = OnPop(qr.Qd, ref state, in qr.ScatterGatherArray) && !qr.ScatterGatherArray.IsEmpty;
                        break;
                    case Opcode.Push:
                        OnPush(qr.Qd, ref state);
                        break;
                }
                if (beginRead)
                {
                    // immediately begin a pop; we already know we have space in the array
                    long qt = 0;
                    LibDemikernel.pop(&qt, readSocket).AssertSuccess(nameof(LibDemikernel.pop));
                    GrowIfNeeded();
                    _liveOperations[_liveOperationCount] = qt;
                    _liveStates[_liveOperationCount++] = state;

                    if (qr.Opcode == Opcode.Accept)
                    {
                        // accept again, making sure we grow if needed
                        qt = 0;
                        LibDemikernel.accept(&qt, qr.Qd).AssertSuccess(nameof(LibDemikernel.accept));
                        GrowIfNeeded();
                        _liveOperations[_liveOperationCount] = qt;
                        _liveStates[_liveOperationCount++] = origState;
                    }
                }
            }
            PrepareForMessagePumpExit(true);
            _shutdown.TrySetResult();
        }
        catch (Exception ex)
        {
            Debug.WriteLine(ex.Message);
            PrepareForMessagePumpExit(false);
            _shutdown.TrySetException(ex);
        }
    }

    private void PrepareForMessagePumpExit(bool assertCleanShutdown)
    {
        foreach (var socket in _liveSockets)
        {
            var apiResult = LibDemikernel.close(socket);
            if (assertCleanShutdown) apiResult.AssertSuccess(nameof(LibDemikernel.close));
        }
        _liveSockets.Clear();
        Volatile.Write(ref s_running, 0);
        _messagePumpThreadId = -1;
    }

    private bool IsMessagePumpThread => _messagePumpThreadId == Environment.CurrentManagedThreadId;

    protected virtual void OnStart() { }
    protected virtual bool OnPop(int socket, ref TState state, in ScatterGatherArray payload)
    {
        if (payload.IsEmpty) Close(socket);
        payload.Dispose();
        return false; // true===read again
    }
    protected virtual void OnPush(int socket, ref TState state) { }
    protected virtual bool OnConnect(int socket, ref TState state) => true; // true===start reading
    protected virtual bool OnAccept(int socket, ref TState state, in AcceptResult accept) => true; // true===start reading & accept again

    // requests a read operation from the specified socket
    private void Pop(int socket, TState state)
    {
        var pending = PendingOperation.Pop(socket);
        if (IsMessagePumpThread)
        {
            AddAsMessagePump(state, in pending);
        }
        else
        {
            AddPending(state, in pending);
        }
    }

    protected void Close(int socket)
    {
        AssertMessagePump();
        if (_liveSockets.Remove(socket))
        {
            LibDemikernel.close(socket).AssertSuccess(nameof(LibDemikernel.close));
        }
    }

    // performs a push operation to the specified socket, and releases the payload once written (which may not be immediately)
    protected unsafe void Push(int socket, TState state, in ScatterGatherArray payload)
    {
        var pending = PendingOperation.Push(socket, payload);
        if (IsMessagePumpThread)
        {
            AddAsMessagePump(state, in pending);
        }
        else
        {
            AddPending(state, in pending);
        }
    }

    private void AddAsMessagePump(TState state, in PendingOperation value)
    {
        GrowIfNeeded();
        _liveOperations[_liveOperationCount] = value.ExecuteDirect();
        _liveStates[_liveOperationCount++] = state;
    }

    private void GrowIfNeeded()
    {
        if (_liveOperationCount == _liveOperations.Length) Grow();
    }
    private void Grow()
    {
        int newLen;
        checked
        {
            newLen = (int)BitOperations.RoundUpToPowerOf2((uint)_liveOperationCount + 1);
        }
        Array.Resize(ref _liveOperations, newLen);
        Array.Resize(ref _liveStates, newLen);
    }

    private void AddPending(TState state, in PendingOperation value)
    {
        lock (_pendingOperations)
        {
            if (!_keepRunning) ThrowDoomed();
            _pendingOperations.Enqueue((state, value));
            _pendingWork = true;
            if (_pendingOperations.Count == 1) Monitor.PulseAll(_pendingOperations);
        }

        static void ThrowDoomed() => throw new InvalidOperationException("The message pump is shutting down");
    }

    private void AssertMessagePump([CallerMemberName] string caller = "")
    {
        if (!IsMessagePumpThread) Throw(caller);
        static void Throw(string caller) => throw new InvalidOperationException($"The {caller} method must only be called from the message-pump");
    }

    protected unsafe int Accept(TState state, AddressFamily addressFamily, SocketType socketType, ProtocolType protocol, EndPoint endPoint, int backlog, int acceptCount = 1)
    {
        AssertMessagePump();

        var addr = endPoint.Serialize();
        var len = addr.Size;
        if (len > 128) throw new InvalidOperationException($"Endpoint address too large: {len} bytes");
        byte* saddr = stackalloc byte[len];
        for (int i = 0; i < len; i++)
        {
            saddr[i] = addr[i];
        }

        int socket = 0;
        LibDemikernel.socket(&socket, addressFamily, socketType, protocol).AssertSuccess(nameof(LibDemikernel.socket));
        if (!_liveSockets.Add(socket)) throw new InvalidOperationException("Duplicate socket: " + socket);

        LibDemikernel.bind(socket, saddr, len).AssertSuccess(nameof(LibDemikernel.bind));
        LibDemikernel.listen(socket, backlog).AssertSuccess(nameof(LibDemikernel.listen));

        for (int i = 0; i < acceptCount; i++)
        {
            long qt = 0;
            LibDemikernel.accept(&qt, socket).AssertSuccess(nameof(LibDemikernel.accept));

            GrowIfNeeded();
            _liveOperations[_liveOperationCount] = qt;
            _liveStates[_liveOperationCount++] = state;
        }
        return socket;
    }
}
