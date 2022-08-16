// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

namespace Example
{
    using System;
    using System.Diagnostics;
    using System.Net;
    using System.Net.Sockets;

    public class UdpPushPop
    {
        public const int MAX_ITERATIONS = 1024;

        public const int DATA_SIZE = 64;

        public static void Server(string[] args, IPEndPoint local)
        {
            int sockqd = -1;

            // Initialize Demikernel.
            Demikernel.Libos.Init(args);

            // Serialize socket address.
            System.Net.SocketAddress localAddress = local.Serialize();

            // Setup socket.
            Demikernel.Libos.Socket(ref sockqd, AddressFamily.InterNetwork, SocketType.Dgram, ProtocolType.Udp);
            Demikernel.Libos.Bind(sockqd, localAddress);

            // Run.
            for (int it = 0; it < UdpPushPop.MAX_ITERATIONS; it++)
            {
                long qt = -1;
                Demikernel.QueueResult qr = new Demikernel.QueueResult();

                // Pop data.
                Demikernel.Libos.Pop(ref qt, sockqd);

                // Wait for pop operation to complete.
                Demikernel.Libos.Wait(ref qr, qt);

                // Parse operation result.
                Debug.Assert(qr.opcode == Demikernel.Opcode.Pop);
                Debug.Assert(qr.value.sga.numsegs == 1);
                Demikernel.ScatterGatherSegment seg = qr.value.sga.segs[0];

                // Check payload.
                Debug.Assert(seg.Length == DATA_SIZE);
                for (uint i = 0; i < seg.Length; i++)
                    Debug.Assert(seg[i] == 1);


                // Release scatter-gather array.
                Demikernel.ScatterGatherArray.Free(ref qr.value.sga);

                Console.WriteLine("pop " + it);
            }
        }

        public static void Client(string[] args, IPEndPoint local, IPEndPoint remote)
        {
            int sockqd = -1;

            // Initialize Demikernel.
            Demikernel.Libos.Init(args);

            // Serialize socket addresses.
            System.Net.SocketAddress localAddress = local.Serialize();
            System.Net.SocketAddress remoteAddress = remote.Serialize();

            // Setup socket.
            Demikernel.Libos.Socket(ref sockqd, AddressFamily.InterNetwork, SocketType.Dgram, ProtocolType.Udp);
            Demikernel.Libos.Bind(sockqd, localAddress);

            // Run.
            for (int it = 0; it < UdpPushPop.MAX_ITERATIONS; it++)
            {
                long qt = -1;
                Demikernel.QueueResult qr = new Demikernel.QueueResult();

                // Allocate scatter-gather array.
                Demikernel.ScatterGatherArray sga = Demikernel.ScatterGatherArray.Alloc(DATA_SIZE);

                // Cook data.
                Demikernel.ScatterGatherSegment seg = sga.segs[0];
                for (uint i = 0; i < seg.Length; i++)
                    seg[i] = 1;

                // Push data.
                Demikernel.Libos.PushTo(ref qt, sockqd, ref sga, remoteAddress);

                // Wait for pop operation to complete.
                Demikernel.Libos.Wait(ref qr, qt);

                // PArse operation result.
                Debug.Assert(qr.opcode == Demikernel.Opcode.Push);

                // Release scatter-gather array.
                Demikernel.ScatterGatherArray.Free(ref sga);

                Console.WriteLine("push " + it);
            }
        }

        // Prints program usage.
        static void usage()
        {
            Console.WriteLine("Usage: " + AppDomain.CurrentDomain.FriendlyName + " MODE local-ipv4 local-port [remote-ipv4] [remote-port]");
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
                IPEndPoint local = buildAddress(args[1], args[2]);

                if (args[0] == "--server")
                {
                    Server(args, local);
                }
                else if ((args.Length >= 4) && (args[0] == "--client"))
                {
                    IPEndPoint remote = buildAddress(args[3], args[4]);
                    Client(args, local, remote);
                }
            }
            else
            {
                usage();
            }
        }
    }
}
