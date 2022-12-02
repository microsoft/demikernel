using System.Runtime.CompilerServices;
using System.Runtime.InteropServices;

namespace Demikernel.Interop;

/// <summary>
/// Represents a set of segments allocated as a buffer for a read or write operation
/// </summary>
/// <remarks>The choice of public fields here is intentional, as this type is a raw P/Invoke layer</remarks>
[StructLayout(LayoutKind.Explicit, Pack = 1, Size = 48)]
public unsafe readonly struct ScatterGatherArray : IDisposable
{
    private const int MAX_SEGMENTS = 1;
    [FieldOffset(0)] private readonly void* buf;
    /// <summary>
    /// The number of segments represented by this buffer
    /// </summary>
    [FieldOffset(8)] public readonly uint SegmentCount;
    [FieldOffset(16)] private readonly ScatterGatherSegment _firstSegment; // [Sizes.SCATTER_GATHER_SEGMENT * MAX_SEGMENTS];
    [FieldOffset(32)] private readonly byte _saddrStart; //[Sizes.SOCKET_ADDRESS];

    static void ThrowMultiSegmentNotExpected()
        => throw new NotSupportedException("Multi-segment buffers not currently anticipated");


    /// <summary>
    /// Restrict this buffer to a sub-range; note that this is a transient value that should not be independently released by <see cref="Dispose"/>,
    /// as it still represents the same underlying data portion.
    /// </summary>
    public ScatterGatherArray Slice(uint start, uint length)
    {
        static void ThrowOutOfRange() => throw new ArgumentOutOfRangeException(nameof(length));
        var currentLen = TotalBytes;
        if (start + length > currentLen) ThrowOutOfRange();
        if (length == 0) return default;

        if (SegmentCount != 1) ThrowMultiSegmentNotExpected();

        var result = this; // copy
        var typed = &result._firstSegment;
        *typed = typed->UncheckedSlice(start, length);
        return result;
    }

    /// <summary>
    /// Indicates whether this buffer is empty (zero bytes)
    /// </summary>
    public bool IsEmpty => SegmentCount switch
    {
        0 => true,
        1 => _firstSegment.Length == 0,
        _ => IsEmptySlow(),
    };
    private bool IsEmptySlow()
    {
        ThrowMultiSegmentNotExpected(); // but impl shown for future ref
        fixed (ScatterGatherSegment* segs = &_firstSegment)
        {
            for (int i = 0; i < SegmentCount; i++)
            {
                if (segs[i].Length != 0) return false;
            }
        }
        return true;
    }

    /// <summary>
    /// Indicates whether this buffer has exactly one segment
    /// </summary>
    public bool IsSingleSegment => SegmentCount < 2;

    /// <summary>
    /// Gets the span of the first segment associated with the buffer
    /// </summary>
    public Span<byte> FirstSpan => SegmentCount switch
    {
        0 => default,
        _ => _firstSegment.Span,
    };

    /// <summary>
    /// Gets the total bytes represented by this buffer
    /// </summary>
    public uint TotalBytes => SegmentCount switch
    {
        0 => 0,
        1 => _firstSegment.Length,
        _ => TotalBytesSlow(),
    };

    private uint TotalBytesSlow()
    {
        ThrowMultiSegmentNotExpected(); // but impl shown for future ref
        uint total = 0;
        fixed (ScatterGatherSegment* segs = &_firstSegment)
        {
            for (int i = 0; i < SegmentCount; i++)
            {
                total += segs[i].Length;
            }
        }
        return total;
    }

    /// <summary>
    /// Gets the span of the specified buffer
    /// </summary>
    public Span<byte> this[int index]
        => index == 0 & SegmentCount != 0 ? _firstSegment.Span : IndexerSlow(index);

    private Span<byte> IndexerSlow(int index)
    {
        if (index < 0 || index >= SegmentCount) Throw();
        ThrowMultiSegmentNotExpected(); // but impl shown for future ref

        fixed (ScatterGatherSegment* segs = &_firstSegment)
        {
            return segs[index].Span;
        }
        static void Throw() => throw new IndexOutOfRangeException();
    }

    /// <inheritdoc/>
    public override string ToString()
        => $"{TotalBytes} bytes over {SegmentCount} segments";

    /// <summary>
    /// Release resources associated with this instance
    /// </summary>
    public void Dispose()
    {
        if (!IsEmpty)
        {
            ApiResult result;
            fixed (ScatterGatherArray* ptr = &this)
            {
                lock (LibDemikernel.GlobalLock)
                {
                    result = LibDemikernel.sgafree(ptr);
                }
            }
            result.AssertSuccess(nameof(LibDemikernel.sgafree));
            Unsafe.AsRef(in this) = default; // pure evil, but: we do what we can
        }
    }

    /// <summary>
    /// An empty buffer
    /// </summary>
    public static readonly ScatterGatherArray Empty;

    /// <summary>
    /// Allocates a buffer of the requested size
    /// </summary>
    public static ScatterGatherArray Create(uint size)
    {
        if (size == 0) return default;
        ScatterGatherArray sga;
        lock (LibDemikernel.GlobalLock)
        {
            sga = LibDemikernel.sgaalloc(size);
        }

        if (sga.SegmentCount == 0 | sga.SegmentCount > MAX_SEGMENTS) Throw(sga.SegmentCount, size);

        static void Throw(uint numsegs, uint size) =>
            throw new InvalidOperationException($"Invalid segment count: {numsegs} ({nameof(Create)} requested {size} bytes)");
        return sga;
    }

    /// <summary>
    /// Attempt to copy data into the buffer
    /// </summary>
    public bool TryCopyFrom(ReadOnlySpan<byte> source)
    {
        if (source.IsEmpty) return true;
        if (IsSingleSegment) return source.TryCopyTo(FirstSpan);
        return TrySlowCopyFrom(source);
    }
    private bool TrySlowCopyFrom(ReadOnlySpan<byte> source)
    {
        for (int i = 0; i < SegmentCount; i++)
        {
            var available = this[i];
            if (available.Length >= source.Length)
            {
                // all fits
                source.CopyTo(available);
                return true;
            }
            // partial fit
#pragma warning disable IDE0057 // opiniated slice usage
            source.Slice(0, available.Length).CopyTo(available);
            source = source.Slice(available.Length);
#pragma warning restore IDE0057 // opiniated slice usage
        }
        return source.IsEmpty;
    }

    internal static ScatterGatherArray Create(ReadOnlySpan<byte> payload)
    {
        var sga = Create((uint)payload.Length);
        if (!sga.TryCopyFrom(payload))
        {
            sga.Dispose();
            Throw();
        }
        return sga;

        static void Throw() => throw new InvalidOperationException("Unable to copy payload to ScatterGatherArray");
    }
}