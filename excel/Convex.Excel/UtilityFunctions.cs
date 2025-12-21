using System;
using ExcelDna.Integration;

namespace Convex.Excel
{
    /// <summary>
    /// Excel UDFs for object management and utilities.
    /// </summary>
    public static class UtilityFunctions
    {
        /// <summary>
        /// Gets the library version.
        /// </summary>
        [ExcelFunction(
            Name = "CX.VERSION",
            Description = "Returns Convex library version",
            Category = "Convex Utilities")]
        public static string CxVersion()
        {
            try
            {
                return ConvexWrapper.GetVersion();
            }
            catch
            {
                return "ERROR";
            }
        }

        /// <summary>
        /// Gets the count of registered objects.
        /// </summary>
        [ExcelFunction(
            Name = "CX.OBJECT.COUNT",
            Description = "Returns count of registered objects",
            Category = "Convex Utilities")]
        public static int CxObjectCount()
        {
            try
            {
                return ConvexWrapper.ObjectCount();
            }
            catch
            {
                return -1;
            }
        }

        /// <summary>
        /// Releases an object by handle.
        /// </summary>
        [ExcelFunction(
            Name = "CX.RELEASE",
            Description = "Releases an object by handle",
            Category = "Convex Utilities")]
        public static object CxRelease(
            [ExcelArgument(Description = "Object handle or name")] object reference)
        {
            try
            {
                ulong handle = HandleHelper.Parse(reference);
                if (handle == NativeMethods.INVALID_HANDLE)
                    return false;

                return ConvexWrapper.Release(handle);
            }
            catch
            {
                return false;
            }
        }

        /// <summary>
        /// Clears all registered objects.
        /// </summary>
        [ExcelFunction(
            Name = "CX.CLEAR.ALL",
            Description = "Clears all registered objects",
            Category = "Convex Utilities")]
        public static string CxClearAll()
        {
            try
            {
                ConvexWrapper.ClearAll();
                return "OK";
            }
            catch (Exception ex)
            {
                return "ERROR: " + ex.Message;
            }
        }

        /// <summary>
        /// Looks up an object handle by name.
        /// </summary>
        [ExcelFunction(
            Name = "CX.LOOKUP",
            Description = "Looks up object handle by name",
            Category = "Convex Utilities")]
        public static object CxLookup(
            [ExcelArgument(Description = "Object name")] string name)
        {
            try
            {
                ulong handle = ConvexWrapper.Lookup(name);
                return handle == NativeMethods.INVALID_HANDLE
                    ? (object)ExcelError.ExcelErrorRef
                    : HandleHelper.Format(handle);
            }
            catch
            {
                return ExcelError.ExcelErrorValue;
            }
        }

        /// <summary>
        /// Gets the type of an object.
        /// </summary>
        [ExcelFunction(
            Name = "CX.TYPE",
            Description = "Gets the type of an object",
            Category = "Convex Utilities")]
        public static string CxType(
            [ExcelArgument(Description = "Object handle or name")] object reference)
        {
            try
            {
                ulong handle = HandleHelper.Parse(reference);
                if (handle == NativeMethods.INVALID_HANDLE)
                    return "INVALID";

                var objType = ConvexWrapper.GetObjectType(handle);
                return objType.ToString();
            }
            catch
            {
                return "ERROR";
            }
        }

        /// <summary>
        /// Gets the last error message.
        /// </summary>
        [ExcelFunction(
            Name = "CX.LAST.ERROR",
            Description = "Gets the last error message",
            Category = "Convex Utilities")]
        public static string CxLastError()
        {
            try
            {
                return ConvexWrapper.GetLastError();
            }
            catch (Exception ex)
            {
                return ex.Message;
            }
        }

        /// <summary>
        /// Gets native library load status.
        /// </summary>
        [ExcelFunction(
            Name = "CX.LOAD.STATUS",
            Description = "Gets native library load status",
            Category = "Convex Utilities")]
        public static string CxLoadStatus()
        {
            try
            {
                NativeLoader.Initialize();
                return NativeLoader.GetLoadError();
            }
            catch (Exception ex)
            {
                return "Load error: " + ex.Message;
            }
        }

    }
}
