using Demikernel.Interop;
using System.Diagnostics;

namespace Demikernel.Internal;

internal sealed class DemiKernelStream : Stream
{
    private readonly Socket _socket;
    private readonly bool _ownsSocket;
    private ScatterGatherArray _readBuffer;
    uint _readOffset, _readRemaining;
    public DemiKernelStream(Socket socket, bool ownsSocket = true)
    {
        _socket = socket;
        _ownsSocket = ownsSocket;
    }

    public override long Seek(long offset, SeekOrigin origin)
        => throw new NotSupportedException();
    public override bool CanRead => true;
    public override bool CanWrite => true;
    public override void Close()
    {
        if (_ownsSocket) _socket.Close();
    }
    public override long Length => throw new NotSupportedException();
    public override void SetLength(long value) => throw new NotSupportedException();
    public override int ReadTimeout
    {
        get => throw new NotSupportedException();
        set => throw new NotSupportedException();
    }
    public override int WriteTimeout
    {
        get => throw new NotSupportedException();
        set => throw new NotSupportedException();
    }

    public override bool CanTimeout => false;
    public override bool CanSeek => false;
    public override long Position
    {
        get => throw new NotSupportedException();
        set => throw new NotSupportedException();
    }
    public override void Flush() { }
    public override Task FlushAsync(CancellationToken cancellationToken) => Task.CompletedTask;

    protected override void Dispose(bool disposing)
    {
        if (disposing)
        {
            GC.SuppressFinalize(this);
            _readBuffer.Dispose();
        }
    }
    public override ValueTask DisposeAsync()
    {
        Dispose(true);
        return default;
    }

    public override int Read(byte[] buffer, int offset, int count) => Read(new Span<byte>(buffer, offset, count));

    public override int Read(Span<byte> buffer)
    {
        if (_readRemaining == 0 && !ReadCore()) return 0;
        return CopyFromBuffer(buffer);
    }

    public override int ReadByte()
    {
        Span<byte> buffer = stackalloc byte[1];
        return Read(buffer) == 0 ? -1 : buffer[0];
    }

    public override Task<int> ReadAsync(byte[] buffer, int offset, int count, CancellationToken cancellationToken)
        => ReadAsync(new Memory<byte>(buffer, offset, count), cancellationToken).AsTask();

    public override ValueTask<int> ReadAsync(Memory<byte> buffer, CancellationToken cancellationToken = default)
    {
        if (_readRemaining != 0) return new(CopyFromBuffer(buffer.Span));
        return ReadAsyncSlow(buffer, cancellationToken);
    }
    private async ValueTask<int> ReadAsyncSlow(Memory<byte> buffer, CancellationToken cancellationToken)
    {
        if (!await ReadCoreAsync(cancellationToken)) return 0;
        return CopyFromBuffer(buffer.Span);
    }

    int CopyFromBuffer(Span<byte> destination)
    {
        var toCopy = (uint)Math.Min(_readRemaining, destination.Length);
        _readBuffer.Slice(_readOffset, toCopy).FirstSpan.CopyTo(destination);
        _readOffset += toCopy;
        _readRemaining -= toCopy;
        if (_readRemaining == 0) _readBuffer.Dispose();
        return (int)toCopy;
    }

    private bool ReadCore()
    {
        Debug.Assert(_readRemaining == 0);
        _readBuffer = _socket.Receive();
        _readOffset = 0;
        _readRemaining = _readBuffer.TotalBytes;
        if (_readRemaining == 0)
        {
            _readBuffer.Dispose();
            return false;
        }
        return true;
    }
    private async ValueTask<bool> ReadCoreAsync(CancellationToken cancellationToken)
    {
        Debug.Assert(_readRemaining == 0);
        _readBuffer = await _socket.ReceiveAsync(cancellationToken);
        _readOffset = 0;
        _readRemaining = _readBuffer.TotalBytes;
        if (_readRemaining == 0)
        {
            _readBuffer.Dispose();
            return false;
        }
        return true;
    }

    public override void WriteByte(byte value)
    {
        ReadOnlySpan<byte> buffer = stackalloc byte[1] { value };
        Write(buffer);
    }

    public override void Write(byte[] buffer, int offset, int count)
        => Write(new ReadOnlySpan<byte>(buffer, offset, count));

    public override Task WriteAsync(byte[] buffer, int offset, int count, CancellationToken cancellationToken)
        => WriteAsync(new ReadOnlyMemory<byte>(buffer, offset, count), cancellationToken).AsTask();

    public override void Write(ReadOnlySpan<byte> buffer) => _socket.Send(buffer);
    public override ValueTask WriteAsync(ReadOnlyMemory<byte> buffer, CancellationToken cancellationToken = default)
        => _socket.SendAsync(buffer.Span, cancellationToken);

    // TODO: CopyTo[Async]
}
