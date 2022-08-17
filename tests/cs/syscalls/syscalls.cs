// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

namespace Test
{
    using System.Net;
    using System.Net.Sockets;

    public class InvalidSystemCalls
    {
        // Issues an invalid call to Demikernel.Socket().
        public static void InvalidSocket()
        {
            int qd = -1;
            AddressFamily domain = AddressFamily.InterNetwork;
            SocketType type = SocketType.Stream;
            ProtocolType protocol = ProtocolType.Tcp;

            Demikernel.Libos.Socket(ref qd, domain, type, protocol);
        }

        // Issues an invalid call to Demikernel.Listen().
        public static void InvalidListen()
        {
            int qd = -1;
            int backlog = -1;

            Demikernel.Libos.Listen(qd, backlog);
        }

        // Issues an invalid call to Demikernel.Bind().
        public static void InvalidBind()
        {
            int qd = -1;
            IPAddress ipAddress = Dns.GetHostEntry("www.microsoft.com").AddressList[0];
            IPEndPoint ipLocalEndPoint = new IPEndPoint(ipAddress, 11000);
            SocketAddress saddr = ipLocalEndPoint.Serialize();

            Demikernel.Libos.Bind(qd, saddr);
        }

        // Issues an invalid call to Demikernel.Accept().
        public static void InvalidAccept()
        {
            long qt = -1;
            int qd = -1;

            Demikernel.Libos.Accept(ref qt, qd);
        }

        // Issues an invalid call to Demikernel.Connect().
        public static void InvalidConnect()
        {
            long qt = -1;
            int qd = -1;
            IPAddress ipAddress = Dns.GetHostEntry("www.microsoft.com").AddressList[0];
            IPEndPoint ipLocalEndPoint = new IPEndPoint(ipAddress, 11000);
            SocketAddress saddr = ipLocalEndPoint.Serialize();

            Demikernel.Libos.Connect(ref qt, qd, saddr);
        }

        // Issues an invalid call to Demikernel.Close().
        public static void InvalidClose()
        {
            int qd = -1;

            Demikernel.Libos.Close(qd);
        }

        // Issues an invalid call to Demikernel.Push().
        public static void InvalidPush()
        {
            long qt = -1;
            int qd = -1;
            Demikernel.ScatterGatherArray sga = new Demikernel.ScatterGatherArray();

            Demikernel.Libos.Push(ref qt, qd, ref sga);
        }

        // Issues an invalid call to Demikernel.PushTo().
        public static void InvalidPushTo()
        {
            long qt = -1;
            int qd = -1;
            Demikernel.ScatterGatherArray sga = new Demikernel.ScatterGatherArray();
            IPAddress ipAddress = Dns.GetHostEntry("www.microsoft.com").AddressList[0];
            IPEndPoint ipLocalEndPoint = new IPEndPoint(ipAddress, 11000);
            SocketAddress saddr = ipLocalEndPoint.Serialize();

            Demikernel.Libos.PushTo(ref qt, qd, ref sga, saddr);
        }

        // Issues an invalid call to Demikernel.Pop().
        public static void InvalidPop()
        {
            long qt = -1;
            int qd = -1;

            Demikernel.Libos.Pop(ref qt, qd);
        }

        // Issues an invalid call to Demikernel.ScatterGather.Alloc().
        public static void InvalidScatterGatherAlloc()
        {
            uint size = 0;

            Demikernel.ScatterGatherArray.Alloc(size);
        }

        // Issues an invalid call to Demikernel.ScatterGather.Free().
        public static void InvalidScatterGatherFree()
        {
            Demikernel.ScatterGatherArray sga = new Demikernel.ScatterGatherArray();

            Demikernel.ScatterGatherArray.Free(ref sga);
        }

        // Issues an invalid call to Demikernel.Wait().
        public static void InvalidWait()
        {
            long qt = -1;
            Demikernel.QueueResult qr = new Demikernel.QueueResult();

            Demikernel.Libos.Wait(ref qr, qt);
        }

        // Issues an invalid call to Demikernel.WaitAny().
        public static void InvalidWaitAny()
        {
            Demikernel.QueueResult qr = new Demikernel.QueueResult();
            int offset = -1;
            long[] qts = new long[1];

            Demikernel.Libos.WaitAny(ref qr, ref offset, ref qts);

        }

        // Main Method
        static void Main(string[] args)
        {
            Demikernel.Libos.Init(args);

            InvalidSocket();
            InvalidListen();
            InvalidBind();
            InvalidAccept();
            InvalidConnect();
            InvalidClose();
            InvalidPush();
            InvalidPushTo();
            InvalidScatterGatherAlloc();
            InvalidScatterGatherFree();
            InvalidWait();
            InvalidWaitAny();
        }
    }
}
