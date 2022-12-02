using Demikernel.Interop;
using System.Diagnostics;
using System.Numerics;
using System.Runtime.CompilerServices;
using System.Runtime.InteropServices;

namespace Demikernel.Internal;

/// <summary>
/// The <see cref="PendingQueue"/> co-ordinates multiple outstanding pending async operations, acting as
/// a driver for `demi_wait_any`; all items added to a single manager instance become part of the same
/// `demi_wait_any` invoke, with new items added as soon as possible (allowing for any existing
/// `demi_wait_any` call to complete first); only the live worker has access to the live queue
/// </summary>
internal static class PendingQueue
{
    private readonly static object s_pendingItemsSyncLock = new();
    private readonly static List<(long Token, object Tcs, CancellationTokenRegistration Ctr)> s_pending = new();
    private static bool s_doomed = false, s_started = false;

    private static void Add(long token, object tcs, in CancellationTokenRegistration ctr)
    {
        var tuple = (token, tcs, ctr);
        lock (s_pendingItemsSyncLock)
        {
            if (s_doomed)
            {
                TryCancel(tcs, ctr);
            }
            else
            {
                // we can't change what wait_any is doing, so: add to a holding pen,
                // and add to wait_any on the *next* iteration
                s_pending.Add(tuple);
                if (s_started)
                {
                    if (s_pending.Count == 1) Monitor.Pulse(s_pendingItemsSyncLock); // wake the worker
                }
                else
                {
                    Thread thd = new(Pump);
                    thd.Name = "DemiKernel pump";
                    thd.IsBackground = true;
                    thd.Start();
                    s_started = true;
                }
            }
        }
    }

    static void EnsureCapacity(ref long[] liveTokens, ref (object Tcs, CancellationTokenRegistration Ctr)[] liveCompletions, int oldCount, int newCount)
    {
        newCount = (int)BitOperations.RoundUpToPowerOf2((uint)newCount);
        if (liveTokens is null)
        {
            liveTokens = GC.AllocateUninitializedArray<long>(newCount, pinned: true);
            liveCompletions = new (object, CancellationTokenRegistration)[newCount];
        }
        else if (newCount > liveTokens.Length)
        {
            var newTokens = GC.AllocateArray<long>(newCount, pinned: true);
            var newCompletions = new (object, CancellationTokenRegistration)[newCount];
            if (oldCount != 0)
            {
                new Span<long>(liveTokens, 0, oldCount).CopyTo(newTokens);
                new Span<(object, CancellationTokenRegistration)>(liveCompletions, 0, oldCount).CopyTo(newCompletions);
            }
            liveTokens = newTokens; // drop the old on the floor
            liveCompletions = newCompletions;
        }
    }

    private unsafe static void Pump()
    {
        var liveTokens = Array.Empty<long>();
        var liveCompletions = Array.Empty<(object Tcs, CancellationTokenRegistration Ctr)>();
        int liveCount = 0;
        Debug.WriteLine("[server] entering dedicated work loop");
        var perLoopTimeout = new TimeSpec(0, 1000); // 1 microsecond, entirely made up - no logic here
        try
        {
            Unsafe.SkipInit(out QueueResult qr);

            int offset = 0;
            while (true)
            {
                lock (s_pendingItemsSyncLock)
                {
                    if (s_doomed) break;
                    if (s_pending.Count != 0)
                    {
                        EnsureCapacity(ref liveTokens, ref liveCompletions, liveCount, liveCount + s_pending.Count);
                        foreach (ref readonly var item in CollectionsMarshal.AsSpan(s_pending))
                        {
                            liveTokens[liveCount] = item.Token;
                            liveCompletions[liveCount++] = (item.Tcs, item.Ctr);
                        }
                        s_pending.Clear();
                    }
                }

                if (liveCount == 0)
                {
                    lock (s_pendingItemsSyncLock)
                    {
                        if (s_pending.Count == 0)
                        {
                            Monitor.Wait(s_pendingItemsSyncLock);
                        }
                        continue;
                    }
                }

                // if (offset < 0 | offset >= liveCount) offset = 0; // ensure valid range
                offset = 0;
                //// we're using the pinned heap; we can do this without "fixed"
                var qts = (long*)Unsafe.AsPointer(ref liveTokens[0]);
                ApiResult result;
                lock (LibDemikernel.GlobalLock)
                {
                    result = LibDemikernel.wait_any(&qr, &offset, qts, liveCount, &perLoopTimeout);
                }
                //Debug.WriteLine($"wait_any: got {result}");
                if (result ==  ApiResult.Timeout)
                {
                    continue;
                }

                result.AssertSuccess(nameof(LibDemikernel.wait_any));
                //if (qr.Opcode == Opcode.Invalid || offset < 0)
                //{
                //    // reset drive (perhaps to add new items)
                //    continue;
                //}

                // right, so we're consuming an item; let's juggle the list
                // by moving the *last* item into this space
                var completion = liveCompletions[offset];
                if (liveCount > 1)
                {
                    liveTokens[offset] = liveTokens[liveCount - 1];
                    liveCompletions[offset] = liveCompletions[liveCount - 1];
                }
                // decrement the size and allow the task the be collected
                //(we don't need to clean up the tokens; they're just integers)
                liveCompletions[--liveCount] = default;

                // signal async completion of the pending activity
                completion.Ctr.Unregister();
                TryComplete(completion.Tcs, in qr);
            }
        }
        catch (Exception ex)
        {
            Debug.WriteLine($"[manager]: {ex.Message}");
        }
        finally
        {
            Debug.WriteLine($"[manager]: exiting loop");
            lock (s_pendingItemsSyncLock)
            {
                s_doomed = true;
                foreach (ref var item in CollectionsMarshal.AsSpan(s_pending))
                {
                    TryCancel(item.Tcs, item.Ctr);
                }
                s_pending.Clear();
            }
            for (int i = 0; i < liveCount; i++)
            {
                ref var completion = ref liveCompletions[i];
                TryCancel(completion.Tcs, completion.Ctr);
                completion = default;
            }
            liveCount = 0;
        }
    }

    static void TryComplete(object tcs, in QueueResult qr)
    {

        if (tcs is TaskCompletionSource raw)
        {
            switch (qr.Opcode)
            {
                case Opcode.Push:
                case Opcode.Connect:
                    raw.TrySetResult();
                    break;
                case Opcode.Failed:
                    raw.TrySetException(CreateFailed());
                    break;
                default:
                    raw.TrySetException(qr.CreateUnexpected(Opcode.Push, Opcode.Connect));
                    break;
            }
        }
        else if (tcs is TaskCompletionSource<ScatterGatherArray> sga)
        {
            switch (qr.Opcode)
            {
                case Opcode.Pop:
                    if (!sga.TrySetResult(qr.ScatterGatherArray))
                    {   // already complete (cancellation, etc)
                        qr.ScatterGatherArray.Dispose();
                    }
                    break;
                case Opcode.Failed:
                    sga.TrySetException(CreateFailed());
                    break;
                default:
                    sga.TrySetException(qr.CreateUnexpected(Opcode.Push));
                    break;
            }
        }
        else if (tcs is TaskCompletionSource<AcceptResult> ar)
        {
            switch (qr.Opcode)
            {
                case Opcode.Accept:
                    if (!ar.TrySetResult(qr.AcceptResult))
                    {   // already complete (cancellation, etc)
                        qr.AcceptResult.AsSocket().Dispose();
                    }
                    break;
                case Opcode.Failed:
                    ar.TrySetException(CreateFailed());
                    break;
                default:
                    ar.TrySetException(qr.CreateUnexpected(Opcode.Push));
                    break;
            }
        }
        static Exception CreateFailed() => new IOException();
    }
    static void TryCancel(object tcs, in CancellationTokenRegistration ctr)
    {
        ctr.Unregister();
        if (tcs is TaskCompletionSource raw)
        {
            raw.TrySetCanceled();
        }
        else if (tcs is TaskCompletionSource<ScatterGatherArray> sga)
        {
            sga.TrySetCanceled();
        }
        else if (tcs is TaskCompletionSource<AcceptResult> ar)
        {
            ar.TrySetCanceled();
        }
    }
    static void TryCancel(object? tcs, CancellationToken cancellationToken)
    {
        if (tcs is TaskCompletionSource raw)
        {
            raw.TrySetCanceled(cancellationToken);
        }
        else if (tcs is TaskCompletionSource<ScatterGatherArray> sga)
        {
            sga.TrySetCanceled(cancellationToken);
        }
        else if (tcs is TaskCompletionSource<AcceptResult> ar)
        {
            ar.TrySetCanceled(cancellationToken);
        }
    }

    static readonly Action<object?, CancellationToken> CancelCallback = TryCancel;
    internal static Task AddVoid(in QueueToken pending, CancellationToken cancellationToken = default)
    {
        if (cancellationToken.IsCancellationRequested) return Task.FromCanceled(cancellationToken);
        var tcs = new TaskCompletionSource(TaskCreationOptions.RunContinuationsAsynchronously);
        CancellationTokenRegistration ctr = cancellationToken.CanBeCanceled ? cancellationToken.Register(CancelCallback, tcs) : default;
        Add(pending.qt, tcs, ctr);
        return tcs.Task;
    }
    internal static Task<ScatterGatherArray> AddReceive(in QueueToken pending, CancellationToken cancellationToken = default)
    {
        if (cancellationToken.IsCancellationRequested) return Task.FromCanceled<ScatterGatherArray>(cancellationToken);
        var tcs = new TaskCompletionSource<ScatterGatherArray>(TaskCreationOptions.RunContinuationsAsynchronously);
        CancellationTokenRegistration ctr = cancellationToken.CanBeCanceled ? cancellationToken.Register(CancelCallback, tcs) : default;
        Add(pending.qt, tcs, ctr);
        return tcs.Task;
    }

    internal static Task<AcceptResult> AddAccept(in QueueToken pending, CancellationToken cancellationToken = default)
    {
        if (cancellationToken.IsCancellationRequested) return Task.FromCanceled<AcceptResult>(cancellationToken);
        var tcs = new TaskCompletionSource<AcceptResult>(TaskCreationOptions.RunContinuationsAsynchronously);
        CancellationTokenRegistration ctr = cancellationToken.CanBeCanceled ? cancellationToken.Register(CancelCallback, tcs) : default;
        Add(pending.qt, tcs, ctr);
        return tcs.Task;
    }
}