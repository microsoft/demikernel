using System.Runtime.InteropServices;

namespace Demikernel.Interop;

// note: it is not accidental that this one is internal; we don't seem to need to expose this directly
[StructLayout(LayoutKind.Explicit, Pack = 1, Size = 16)]
internal readonly unsafe struct ScatterGatherSegment
{
    [FieldOffset(0)] private readonly byte* _buf;

    [FieldOffset(8)] private readonly uint _len;

    public Span<byte> Span => new Span<byte>(_buf, checked((int)_len));
    public uint Length => _len;

    public string Raw => new IntPtr(_buf).ToString();

    internal ScatterGatherSegment UncheckedSlice(uint start, uint length)
        => new ScatterGatherSegment(_buf + start, length);

    private ScatterGatherSegment(byte* buf, uint len)
    {
        _buf = buf;
        _len = len;
    }
}
