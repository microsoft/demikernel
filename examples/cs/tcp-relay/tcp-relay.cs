// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

namespace Example
{
    using System;
    using System.Diagnostics;
    using System.Net;
    using System.Net.Sockets;

    public class TcpRelay
    {

        private const int DATA_SIZE = 64;

        private const int SOCKET_LENGTH = 16;

        public static void RelayServer(string[] args, IPEndPoint local, IPEndPoint remote)
        {
            long qt = -1;
            uint nbytes = 0;
            int clientSocket = -1;
            int localSocket = -1;
            int serverSocket = -1;
            Demikernel.QueueResult qr = new Demikernel.QueueResult();

            // Initialize Demikernel.
            Demikernel.Libos.Init(args);

            // Setup remote connection.
            System.Net.SocketAddress serverAddress = remote.Serialize();
            Demikernel.Libos.Socket(ref serverSocket, AddressFamily.InterNetwork, SocketType.Stream, ProtocolType.Tcp);
            Demikernel.Libos.Connect(ref qt, serverSocket, serverAddress);
            Demikernel.Libos.Wait(ref qr, qt);
            if (qr.opcode != Demikernel.Opcode.Connect)
                throw new SystemException("Failed to connect to remote server " + remote);

            // Setup local socket.
            System.Net.SocketAddress localAddress = local.Serialize();
            Demikernel.Libos.Socket(ref localSocket, AddressFamily.InterNetwork, SocketType.Stream, ProtocolType.Tcp);
            Demikernel.Libos.Bind(localSocket, localAddress);
            Demikernel.Libos.Listen(localSocket, SOCKET_LENGTH);

            // Wait for incoming operation.
            Demikernel.Libos.Accept(ref qt, localSocket);
            Demikernel.Libos.Wait(ref qr, qt);
            if (qr.opcode != Demikernel.Opcode.Accept)
                throw new SystemException("Failed to accept incoming client connection.");
            clientSocket = qr.value.ares.qd;

            // Run.
            while (true)
            {
                // Pop data.
                Demikernel.Libos.Pop(ref qt, clientSocket);

                // Wait for pop operation to complete.
                Demikernel.Libos.Wait(ref qr, qt);

                // Parse operation result.
                if (qr.opcode != Demikernel.Opcode.Pop)
                    throw new SystemException("Unexpected operation result");
                Debug.Assert(qr.value.sga.numsegs == 1);

                Demikernel.ScatterGatherArray sga = qr.value.sga;
                Demikernel.ScatterGatherSegment seg = sga.segs[0];

                // Check payload.
                nbytes += seg.Length;
                for (uint i = 0; i < seg.Length; i++)
                    Debug.Assert(seg[i] == 1);

                // Push data.
                Demikernel.Libos.Push(ref qt, serverSocket, ref sga);

                // Wait for pop operation to complete.
                Demikernel.Libos.Wait(ref qr, qt);

                // Parse operation result.
                Debug.Assert(qr.opcode == Demikernel.Opcode.Push);

                // Release scatter-gather array.
                Demikernel.ScatterGatherArray.Free(ref sga);

                Console.WriteLine(nbytes + " bytes relayed");
            }
        }

        // Prints program usage.
        static void usage()
        {
            Console.WriteLine("Usage: " + AppDomain.CurrentDomain.FriendlyName + " local-ipv4 local-port remote-ipv4 remote-port");
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
            if (args.Length == 4)
            {
                IPEndPoint local = buildAddress(args[0], args[1]);
                IPEndPoint remote = buildAddress(args[2], args[3]);

                RelayServer(args, local, remote);
            }
            else
                usage();
        }
    }
}

