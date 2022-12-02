using Demikernel.Interop;
using System.Diagnostics.CodeAnalysis;
using System.Runtime.CompilerServices;
using System.Runtime.InteropServices;

namespace Demikernel.Internal;

[StructLayout(LayoutKind.Explicit, Pack = 1, Size = sizeof(long))]
internal readonly struct QueueToken : IEquatable<QueueToken>
{
    [FieldOffset(0)]
    internal readonly long qt;

    internal unsafe void WaitSend()
    {
        Unsafe.SkipInit(out QueueResult qr);
        ApiResult result;
        lock (LibDemikernel.GlobalLock)
        {
            result = LibDemikernel.wait(&qr, this.qt, null);
        }
        result.AssertSuccess(nameof(LibDemikernel.wait));
        qr.Assert(Opcode.Push);
    }

    internal unsafe AcceptResult WaitAccept()
    {
        Unsafe.SkipInit(out QueueResult qr);
        ApiResult result;
        lock (LibDemikernel.GlobalLock)
        {
            result = LibDemikernel.wait(&qr, this.qt, null);
        }
        result.AssertSuccess(nameof(LibDemikernel.wait));
        qr.Assert(Opcode.Accept);
        return qr.AcceptResult;
    }

    internal unsafe ScatterGatherArray WaitReceive()
    {
        Unsafe.SkipInit(out QueueResult qr);
        ApiResult result;
        lock (LibDemikernel.GlobalLock)
        {
            result = LibDemikernel.wait(&qr, this.qt, null);
        }
        result.AssertSuccess(nameof(LibDemikernel.wait));
        qr.Assert(Opcode.Pop);
        return qr.ScatterGatherArray;
    }

    public override string ToString() => $"Queue-token {qt}";

    public override int GetHashCode() => qt.GetHashCode();

    public override bool Equals([NotNullWhen(true)] object? obj)
        => obj is QueueToken other && other.qt == qt;

    public bool Equals(QueueToken other) => other.qt == qt;
}