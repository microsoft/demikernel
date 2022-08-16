
// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

namespace Demikernel
{
    using System;
    using System.Runtime.InteropServices;
    using System.Net;
    using System.Net.Sockets;

    using QueueToken = System.Int64;

    // Size for various structures.
    struct Sizes
    {
        public const int SOCKET_ADDRESS = 16;
        public const int SCATTER_GATHER_SEGMENT = 16;
        public const int SCATTER_GATHER_ARRAY = 48;
        public const int ACCEPT_RESULT = 20;
        public const int QUEUE_RESULT_VALUE = 48;
        public const int QUEUE_RESULT = 64;
    }

    [StructLayout(LayoutKind.Explicit, Pack = 1, Size = 16)]
    unsafe public struct ScatterGatherSegment
    {
        [FieldOffset(0)] private UIntPtr buf_;

        [FieldOffset(8)] private UInt32 len_;

        public byte this[uint offset]
        {
            get
            {
                byte* bptr = (byte*)buf_;
                return bptr[offset];
            }
            set
            {
                byte* bptr = (byte*)buf_;
                bptr[offset] = value;
            }
        }

        public UIntPtr Buffer
        {
            get => buf_;
            set => buf_ = value;
        }

        public uint Length
        {
            get => len_;
            set => len_ = value;
        }
    };

    [StructLayout(LayoutKind.Explicit, Pack = 1)]
    unsafe public struct ScatterGatherArray
    {
        public const int MAX_SEGMENTS = 1;
        [FieldOffset(0)] private UIntPtr buf;
        [FieldOffset(8)] public UInt32 numsegs;
        [FieldOffset(16)] private fixed byte segs_[Sizes.SCATTER_GATHER_SEGMENT * MAX_SEGMENTS];
        [FieldOffset(32)] private fixed byte saddr_[Sizes.SOCKET_ADDRESS];

        public ScatterGatherSegment[] segs
        {
            get
            {
                ScatterGatherSegment[] newBuffer = new ScatterGatherSegment[1];

                fixed (byte* b = segs_)
                {
                    ScatterGatherSegment* bptr = (ScatterGatherSegment*)b;
                    for (int i = 0; i < newBuffer.Length; i++)
                    {
                        newBuffer[i].Buffer = bptr->Buffer;
                        newBuffer[i].Length = bptr->Length;
                        bptr++;
                    }
                }

                return newBuffer;
            }
        }

        public static ScatterGatherArray Alloc(uint size)
        {
            return Demikernel.Interop.sgaalloc(size);
        }

        public static int Free(ref ScatterGatherArray sga)
        {
            return Demikernel.Interop.sgafree(ref sga);
        }
    };

    public enum Opcode
    {
        Invalid = 0,
        Push,
        Pop,
        Accept,
        Connect,
        Failed,
    }

    [StructLayout(LayoutKind.Explicit, Pack = 1, Size = 20)]
    unsafe public struct AcceptResult
    {
        [FieldOffset(0)] public System.Int32 qd;

        [FieldOffset(4)] private fixed byte saddr[Sizes.SOCKET_ADDRESS];
    }

    [StructLayout(LayoutKind.Explicit, Pack = 1)]
    public struct QueueResultValue
    {
        [FieldOffset(0)] public ScatterGatherArray sga;
        [FieldOffset(0)] public AcceptResult ares;
    }

    [StructLayout(LayoutKind.Sequential, Pack = 1)]
    public struct QueueResult
    {
        public Opcode opcode;
        public System.Int32 qd;
        public System.Int64 qt;
        public QueueResultValue value;
    }

    public class Interop
    {

        [DllImport("libdemikernel.so", EntryPoint = "demi_init")]
        public static extern int init(int argc, string[] args);

        [DllImport("libdemikernel.so", EntryPoint = "demi_socket")]
        public static extern int socket(ref int qd, AddressFamily domain, SocketType type, ProtocolType protocol);

        [DllImport("libdemikernel.so", EntryPoint = "demi_listen")]
        public static extern int listen(int qd, int backlog);

        [DllImport("libdemikernel.so", EntryPoint = "demi_bind")]
        public static extern int bind(int qd, byte[] saddr, int size);

        [DllImport("libdemikernel.so", EntryPoint = "demi_accept")]
        public static extern int accept(ref QueueToken qt, int sockqd);

        [DllImport("libdemikernel.so", EntryPoint = "demi_connect")]
        public static extern int connect(ref QueueToken qt, int qd, byte[] saddr, int size);

        [DllImport("libdemikernel.so", EntryPoint = "demi_close")]
        public static extern int close(int qd);

        [DllImport("libdemikernel.so", EntryPoint = "demi_push")]
        internal static extern int push(ref QueueToken qt, int qd, ref ScatterGatherArray sga);

        [DllImport("libdemikernel.so", EntryPoint = "demi_pushto")]
        internal static extern int pushto(ref QueueToken qt, int qd, ref ScatterGatherArray sga, byte[] saddr, int size);

        [DllImport("libdemikernel.so", EntryPoint = "demi_pop")]
        public static extern int pop(ref QueueToken qt, int qd);

        [DllImport("libdemikernel.so", EntryPoint = "demi_wait")]
        public static extern int wait(ref QueueResult qr, QueueToken qt);

        [DllImport("libdemikernel.so", EntryPoint = "demi_wait_any")]
        public static extern int wait_any(ref QueueResult qr, ref int offset, QueueToken[] qt, int num_qts);

        [DllImport("libdemikernel.so", EntryPoint = "demi_sgaalloc")]
        public static extern ScatterGatherArray sgaalloc(ulong size);

        [DllImport("libdemikernel.so", EntryPoint = "demi_sgafree")]
        public static extern int sgafree(ref ScatterGatherArray sga);
    }

    public class Libos
    {
        // Converts a SocketAddress into a raw buffer.
        private static byte[] SocketAddressToRawBuffer(System.Net.SocketAddress saddr)
        {
            // NOTE: we don't have direct access to SocketAddress.Buffer.
            // TODO: Figure out a better way of doing this.
            int bufferSize = saddr.Size;
            byte[] buffer = new byte[bufferSize];
            for (int i = 0; i < bufferSize; i++)
                buffer[i] = saddr[i];

            return buffer;
        }

        private static void CheckSizes()
        {
            if (Marshal.SizeOf(typeof(Demikernel.ScatterGatherSegment)) != Demikernel.Sizes.SCATTER_GATHER_SEGMENT)
                throw new System.PlatformNotSupportedException("Invalid size for ScatterGatterSegment structure.");
            if (Marshal.SizeOf(typeof(Demikernel.ScatterGatherArray)) != Demikernel.Sizes.SCATTER_GATHER_ARRAY)
                throw new System.PlatformNotSupportedException("Invalid size for ScatterGatterArray structure.");
            if (Marshal.SizeOf(typeof(Demikernel.AcceptResult)) != Demikernel.Sizes.ACCEPT_RESULT)
                throw new System.PlatformNotSupportedException("Invalid size for AcceptResult structure.");
            if (Marshal.SizeOf(typeof(Demikernel.QueueResultValue)) != Demikernel.Sizes.QUEUE_RESULT_VALUE)
                throw new System.PlatformNotSupportedException("Invalid size for QueueResultValue structure.");
            if (Marshal.SizeOf(typeof(Demikernel.QueueResult)) != Demikernel.Sizes.QUEUE_RESULT)
                throw new System.PlatformNotSupportedException("Invalid size for QueueResult structure.");
        }

        // Initializes LibOS State
        public static int Init(string[] args)
        {
            CheckSizes();

            return Interop.init(args.Length, args);
        }

        // Creates a socket.
        public static int Socket(ref int qd, AddressFamily domain, SocketType type, ProtocolType protocol)
        {
            return Interop.socket(ref qd, domain, type, protocol);
        }

        // Listens for connections on a socket.
        public static int Listen(int qd, int backlog)
        {
            return Interop.listen(qd, backlog);
        }

        // Binds a nme to a socket.
        public static int Bind(int qd, SocketAddress saddr)
        {
            byte[] buffer = SocketAddressToRawBuffer(saddr);
            return Interop.bind(qd, buffer, buffer.Length);
        }

        // Accepts a connection on a socket.
        public static int Accept(ref QueueToken qt, int sockqd)
        {
            return Interop.accept(ref qt, sockqd);
        }

        // Initiates a connection on a socket.
        public static int Connect(ref QueueToken qt, int qd, System.Net.SocketAddress saddr)
        {
            byte[] buffer = SocketAddressToRawBuffer(saddr);
            return Interop.connect(ref qt, qd, buffer, buffer.Length);
        }

        // Closes a queue descriptor.
        public static int Close(int qd)
        {
            return Interop.close(qd);
        }

        // Asynchronously pushes data through a socket.
        public static int Push(ref QueueToken qt, int qd, ref ScatterGatherArray sga)
        {
            return Interop.push(ref qt, qd, ref sga);
        }

        // Asynchronously pushes data through a socket.
        public static int PushTo(ref QueueToken qt, int qd, ref ScatterGatherArray sga, System.Net.SocketAddress saddr)
        {
            byte[] buffer = SocketAddressToRawBuffer(saddr);
            return Interop.pushto(ref qt, qd, ref sga, buffer, buffer.Length);
        }

        // Asynchronously pops data from a socket.
        public static int Pop(ref QueueToken qt, int qd)
        {
            return Interop.pop(ref qt, qd);
        }

        // Waits for the completion of a queue operation.
        public static int Wait(ref QueueResult qr, QueueToken qd)
        {
            return Interop.wait(ref qr, qd);
        }

        // Waits for the completion of any queue operation.
        public static int WaitAny(ref QueueResult qr, ref int offset, ref QueueToken[] qts)
        {
            return Interop.wait_any(ref qr, ref offset, qts, qts.Length);
        }
    }
}