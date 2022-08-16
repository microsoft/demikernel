// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

namespace Example
{
    using System;
    using System.Diagnostics;
    using System.Net;
    using System.Net.Sockets;

    public class TcpPushPop
    {
        public const int NUM_ITERATIONS = 1024;

        public const int DATA_SIZE = 64;

        public const int TOTAL_BYTES = NUM_ITERATIONS * DATA_SIZE;

        public static void Server(string[] args, IPEndPoint local)
        {
            long qt = -1;
            int qd = -1;
            int sockqd = -1;
            Demikernel.QueueResult qr = new Demikernel.QueueResult();

            // Initialize Demikernel.
            Demikernel.Libos.Init(args);

            // Serialize socket address.
            System.Net.SocketAddress localAddress = local.Serialize();

            // Setup socket.
            Demikernel.Libos.Socket(ref sockqd, AddressFamily.InterNetwork, SocketType.Stream, ProtocolType.Tcp);
            Demikernel.Libos.Bind(sockqd, localAddress);
            Demikernel.Libos.Listen(sockqd, 16);

            // Wait for incoming operation.
            Demikernel.Libos.Accept(ref qt, sockqd);
            Demikernel.Libos.Wait(ref qr, qt);
            if (qr.opcode != Demikernel.Opcode.Accept)
                throw new SystemException("Unexpected operation result");
            qd = qr.value.ares.qd;

            // Run.
            for (uint nbytes = 0; nbytes < TcpPushPop.TOTAL_BYTES; /* noop*/ )
            {
                // Pop data.
                Demikernel.Libos.Pop(ref qt, qd);

                // Wait for pop operation to complete.
                Demikernel.Libos.Wait(ref qr, qt);

                // Parse operation result.
                if (qr.opcode != Demikernel.Opcode.Pop)
                    throw new SystemException("Unexpected operation result");
                Debug.Assert(qr.value.sga.numsegs == 1);
                Demikernel.ScatterGatherSegment seg = qr.value.sga.segs[0];

                // Check payload.
                nbytes += seg.Length;
                for (uint i = 0; i < seg.Length; i++)
                    Debug.Assert(seg[i] == 1);


                // Release scatter-gather array.
                Demikernel.ScatterGatherArray.Free(ref qr.value.sga);

                Console.WriteLine("pop " + nbytes);
            }
        }

        public static void Client(string[] args, IPEndPoint local)
        {
            long qt = -1;
            int sockqd = -1;
            Demikernel.QueueResult qr = new Demikernel.QueueResult();

            // Initialize Demikernel.
            Demikernel.Libos.Init(args);

            // Serialize socket addresses.
            System.Net.SocketAddress localAddress = local.Serialize();

            // Setup socket.
            Demikernel.Libos.Socket(ref sockqd, AddressFamily.InterNetwork, SocketType.Stream, ProtocolType.Tcp);

            // Connect
            Demikernel.Libos.Connect(ref qt, sockqd, localAddress);
            Demikernel.Libos.Wait(ref qr, qt);
            if (qr.opcode != Demikernel.Opcode.Connect)
                throw new SystemException("Unexpected operation result");

            // Run.
            for (int it = 0; it < TcpPushPop.NUM_ITERATIONS; it++)
            {
                // Allocate scatter-gather array.
                Demikernel.ScatterGatherArray sga = Demikernel.ScatterGatherArray.Alloc(DATA_SIZE);

                // Cook data.
                Demikernel.ScatterGatherSegment seg = sga.segs[0];
                for (uint i = 0; i < seg.Length; i++)
                    seg[i] = 1;

                // Push data.
                Demikernel.Libos.Push(ref qt, sockqd, ref sga);

                // Wait for pop operation to complete.
                Demikernel.Libos.Wait(ref qr, qt);

                // Parse operation result.
                Debug.Assert(qr.opcode == Demikernel.Opcode.Push);

                // Release scatter-gather array.
                Demikernel.ScatterGatherArray.Free(ref sga);

                Console.WriteLine("push " + it);
            }
        }

        // Prints program usage.
        static void usage()
        {
            Console.WriteLine("Usage: " + AppDomain.CurrentDomain.FriendlyName + " MODE ipv4 port");
            Console.WriteLine("Modes:");
            Console.WriteLine("  --client    Launch program in client mode.");
            Console.WriteLine("  --server    Launch program in server mode.");
        }

        // Builds an IPv4 endpoint address.
        static IPEndPoint buildAddress(String ipv4Str, String portStr)
        {
            int port = Int16.Parse(portStr);
            IPAddress ipv4 = System.Net.IPAddress.Parse(ipv4Str);
            IPEndPoint endpoint = new System.Net.IPEndPoint(ipv4, port);

            return endpoint;
        }

        // Main Method
        static void Main(string[] args)
        {
            if (args.Length >= 3)
            {
                IPEndPoint addr = buildAddress(args[1], args[2]);

                if (args[0] == "--server")
                    Server(args, addr);
                else if (args[0] == "--client")
                    Client(args, addr);
            }
            else
                usage();
        }
    }
}
