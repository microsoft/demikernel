using Xunit;

namespace Demikernel.Test;

public abstract class TestBase
{
    public static bool LibraryAvailable { get; }
    static TestBase()
    {
        try
        {
            Socket.Initialize();
            LibraryAvailable = true;
        }
        catch (Exception ex)
        {
            Console.Error.WriteLine(ex.Message);
        }
    }

    protected void RequireNativeLib()
    {
        Skip.IfNot(LibraryAvailable, "Native library not available");
    }
}