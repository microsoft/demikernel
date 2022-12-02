namespace Demikernel.Interop;

/// <summary>
/// The operation associated with a queued result.
/// </summary>
public enum Opcode
{
    /// <summary>
    /// Reuslt has not been initialized
    /// </summary>
    Invalid = 0,
    /// <summary>
    /// Result corresponds to a push (write) operation
    /// </summary>
    Push = 1,
    /// <summary>
    /// Result corresponds to a pop (read) operation
    /// </summary>
    Pop = 2,
    /// <summary>
    /// Result corresponds to a server accept operation
    /// </summary>
    Accept = 3,
    /// <summary>
    /// Result corresponds to a client connect operation
    /// </summary>
    Connect = 4,
    /// <summary>
    /// Operation has failed
    /// </summary>
    Failed = 5,
}