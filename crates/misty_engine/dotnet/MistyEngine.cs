// Misty Engine C# scripting API (proof of concept).
//
// This file is compiled together with a project's `.cs` scripts into one assembly
// (`Library/ScriptAssemblies/<hash>/Assembly-CSharp.dll`). The engine loads that assembly into
// the .NET runtime (hostfxr) and calls the `[UnmanagedCallersOnly]` entry points of
// `Misty.Internal.Host`; scripts reach back into the engine through a table of native function
// pointers handed over in `Init`. See `crates/misty_engine/src/scripting/csharp.rs`.

using System;
using System.Collections.Generic;
using System.Reflection;
using System.Runtime.InteropServices;

namespace Misty
{
    /// <summary>A position, direction or scale, like Unity's Vector3.</summary>
    [StructLayout(LayoutKind.Sequential)]
    public struct Vector3 : IEquatable<Vector3>
    {
        public float x, y, z;

        public Vector3(float x, float y, float z) { this.x = x; this.y = y; this.z = z; }

        public static Vector3 zero => new Vector3(0, 0, 0);
        public static Vector3 one => new Vector3(1, 1, 1);
        public static Vector3 up => new Vector3(0, 1, 0);
        public static Vector3 down => new Vector3(0, -1, 0);
        public static Vector3 right => new Vector3(1, 0, 0);
        public static Vector3 left => new Vector3(-1, 0, 0);
        public static Vector3 forward => new Vector3(0, 0, 1);
        public static Vector3 back => new Vector3(0, 0, -1);

        public float magnitude => MathF.Sqrt(x * x + y * y + z * z);
        public float sqrMagnitude => x * x + y * y + z * z;
        public Vector3 normalized
        {
            get { float m = magnitude; return m > 1e-6f ? this / m : zero; }
        }

        public static Vector3 operator +(Vector3 a, Vector3 b) => new Vector3(a.x + b.x, a.y + b.y, a.z + b.z);
        public static Vector3 operator -(Vector3 a, Vector3 b) => new Vector3(a.x - b.x, a.y - b.y, a.z - b.z);
        public static Vector3 operator -(Vector3 a) => new Vector3(-a.x, -a.y, -a.z);
        public static Vector3 operator *(Vector3 a, float d) => new Vector3(a.x * d, a.y * d, a.z * d);
        public static Vector3 operator *(float d, Vector3 a) => new Vector3(a.x * d, a.y * d, a.z * d);
        public static Vector3 operator /(Vector3 a, float d) => new Vector3(a.x / d, a.y / d, a.z / d);
        public static bool operator ==(Vector3 a, Vector3 b) => (a - b).sqrMagnitude < 1e-10f;
        public static bool operator !=(Vector3 a, Vector3 b) => !(a == b);

        public static float Dot(Vector3 a, Vector3 b) => a.x * b.x + a.y * b.y + a.z * b.z;
        public static Vector3 Cross(Vector3 a, Vector3 b) =>
            new Vector3(a.y * b.z - a.z * b.y, a.z * b.x - a.x * b.z, a.x * b.y - a.y * b.x);
        public static float Distance(Vector3 a, Vector3 b) => (a - b).magnitude;
        public static Vector3 Lerp(Vector3 a, Vector3 b, float t)
        {
            t = Mathf.Clamp01(t);
            return a + (b - a) * t;
        }

        public bool Equals(Vector3 other) => this == other;
        public override bool Equals(object obj) => obj is Vector3 v && this == v;
        public override int GetHashCode() => HashCode.Combine(x, y, z);
        public override string ToString() => $"({x:F2}, {y:F2}, {z:F2})";
    }

    /// <summary>A rotation, like Unity's Quaternion (x, y, z, w).</summary>
    [StructLayout(LayoutKind.Sequential)]
    public struct Quaternion
    {
        public float x, y, z, w;

        public Quaternion(float x, float y, float z, float w) { this.x = x; this.y = y; this.z = z; this.w = w; }

        public static Quaternion identity => new Quaternion(0, 0, 0, 1);

        /// <summary>Rotation from Euler angles in degrees, using the engine's convention.</summary>
        public static Quaternion Euler(float x, float y, float z) => Internal.Native.EulerToQuat(new Vector3(x, y, z));
        public static Quaternion Euler(Vector3 euler) => Internal.Native.EulerToQuat(euler);

        public Vector3 eulerAngles => Internal.Native.QuatToEuler(this);

        public static Quaternion operator *(Quaternion a, Quaternion b) => new Quaternion(
            a.w * b.x + a.x * b.w + a.y * b.z - a.z * b.y,
            a.w * b.y - a.x * b.z + a.y * b.w + a.z * b.x,
            a.w * b.z + a.x * b.y - a.y * b.x + a.z * b.w,
            a.w * b.w - a.x * b.x - a.y * b.y - a.z * b.z);

        public static Vector3 operator *(Quaternion q, Vector3 v)
        {
            var u = new Vector3(q.x, q.y, q.z);
            var t = 2f * Vector3.Cross(u, v);
            return v + q.w * t + Vector3.Cross(u, t);
        }

        public override string ToString() => $"({x:F3}, {y:F3}, {z:F3}, {w:F3})";
    }

    public static class Mathf
    {
        public const float PI = MathF.PI;
        public const float Deg2Rad = MathF.PI / 180f;
        public const float Rad2Deg = 180f / MathF.PI;
        public static float Clamp(float v, float min, float max) => v < min ? min : (v > max ? max : v);
        public static float Clamp01(float v) => Clamp(v, 0f, 1f);
        public static float Lerp(float a, float b, float t) => a + (b - a) * Clamp01(t);
        public static float Sin(float f) => MathF.Sin(f);
        public static float Cos(float f) => MathF.Cos(f);
        public static float Abs(float f) => MathF.Abs(f);
        public static float Sqrt(float f) => MathF.Sqrt(f);
        public static float Min(float a, float b) => MathF.Min(a, b);
        public static float Max(float a, float b) => MathF.Max(a, b);
    }

    /// <summary>The game clock, like Unity's Time class.</summary>
    public static class Time
    {
        public static float deltaTime { get; internal set; }
        public static float time { get; internal set; }
        public static float fixedDeltaTime { get; internal set; } = 0.02f;
        public static long frameCount { get; internal set; }
    }

    /// <summary>Messages to the editor Console.</summary>
    public static class Debug
    {
        public static void Log(object message) => Internal.Native.Log(0, message?.ToString() ?? "null");
        public static void LogWarning(object message) => Internal.Native.Log(1, message?.ToString() ?? "null");
        public static void LogError(object message) => Internal.Native.Log(2, message?.ToString() ?? "null");
    }

    /// <summary>An object in the scene.</summary>
    public sealed class GameObject
    {
        internal readonly ulong id;
        internal GameObject(ulong id) { this.id = id; }

        public string name => Internal.Native.GetName(id);
        public Transform transform => new Transform(id);

        public override bool Equals(object obj) => obj is GameObject g && g.id == id;
        public override int GetHashCode() => id.GetHashCode();
    }

    /// <summary>Position, rotation and scale of an object (local to its parent).</summary>
    public sealed class Transform
    {
        internal readonly ulong id;
        internal Transform(ulong id) { this.id = id; }

        public GameObject gameObject => new GameObject(id);

        public Vector3 localPosition
        {
            get => Internal.Native.Get(id).position;
            set { var t = Internal.Native.Get(id); t.position = value; Internal.Native.Set(id, t); }
        }

        public Quaternion localRotation
        {
            get => Internal.Native.Get(id).rotation;
            set { var t = Internal.Native.Get(id); t.rotation = value; Internal.Native.Set(id, t); }
        }

        public Vector3 localScale
        {
            get => Internal.Native.Get(id).scale;
            set { var t = Internal.Native.Get(id); t.scale = value; Internal.Native.Set(id, t); }
        }

        // The proof has no parenting API yet: position and rotation are the local values.
        public Vector3 position { get => localPosition; set => localPosition = value; }
        public Quaternion rotation { get => localRotation; set => localRotation = value; }
        public Vector3 eulerAngles { get => localRotation.eulerAngles; set => localRotation = Quaternion.Euler(value); }

        public Vector3 forward => localRotation * Vector3.forward;
        public Vector3 right => localRotation * Vector3.right;
        public Vector3 up => localRotation * Vector3.up;

        /// <summary>Rotate by Euler angles in degrees around the object's own axes.</summary>
        public void Rotate(Vector3 eulers)
        {
            var t = Internal.Native.Get(id);
            t.rotation = t.rotation * Quaternion.Euler(eulers);
            Internal.Native.Set(id, t);
        }

        public void Rotate(float x, float y, float z) => Rotate(new Vector3(x, y, z));

        /// <summary>Move by `translation` along the object's own axes.</summary>
        public void Translate(Vector3 translation)
        {
            var t = Internal.Native.Get(id);
            t.position = t.position + t.rotation * translation;
            Internal.Native.Set(id, t);
        }

        public void Translate(float x, float y, float z) => Translate(new Vector3(x, y, z));
    }

    /// <summary>Base class of C# scripts, like Unity's MonoBehaviour. Awake, OnEnable, Start,
    /// Update, FixedUpdate, LateUpdate, OnDisable and OnDestroy are found by name (any
    /// visibility, no parameters).</summary>
    public abstract class MonoBehaviour
    {
        internal ulong entity;
        public GameObject gameObject => new GameObject(entity);
        public Transform transform => new Transform(entity);
        public string name => Internal.Native.GetName(entity);
    }
}

namespace Misty.Internal
{
    [StructLayout(LayoutKind.Sequential)]
    internal struct TransformData
    {
        public Vector3 position;
        public Quaternion rotation;
        public Vector3 scale;
    }

    /// <summary>Function pointers into the engine (filled from Rust, same field order as
    /// `NativeApi` in csharp.rs).</summary>
    [StructLayout(LayoutKind.Sequential)]
    internal unsafe struct NativeApi
    {
        public delegate* unmanaged<int, char*, int, void> log;
        public delegate* unmanaged<ulong, TransformData*, int> getTransform;
        public delegate* unmanaged<ulong, TransformData*, int> setTransform;
        public delegate* unmanaged<Vector3*, Quaternion*, void> eulerToQuat;
        public delegate* unmanaged<Quaternion*, Vector3*, void> quatToEuler;
        public delegate* unmanaged<ulong, char*, int, int> getName;
    }

    internal static unsafe class Native
    {
        internal static NativeApi api;

        internal static void Log(int kind, string text)
        {
            fixed (char* p = text) api.log(kind, p, text.Length);
        }

        internal static TransformData Get(ulong id)
        {
            TransformData t;
            if (api.getTransform(id, &t) == 0) throw new InvalidOperationException("The object was destroyed.");
            return t;
        }

        internal static void Set(ulong id, TransformData t)
        {
            if (api.setTransform(id, &t) == 0) throw new InvalidOperationException("The object was destroyed.");
        }

        internal static Quaternion EulerToQuat(Vector3 e)
        {
            Quaternion q;
            api.eulerToQuat(&e, &q);
            return q;
        }

        internal static Vector3 QuatToEuler(Quaternion q)
        {
            Vector3 e;
            api.quatToEuler(&q, &e);
            return e;
        }

        internal static string GetName(ulong id)
        {
            char* buf = stackalloc char[256];
            int n = api.getName(id, buf, 256);
            return n < 0 ? "" : new string(buf, 0, Math.Min(n, 256));
        }
    }

    /// <summary>Entry points the engine calls. Method ids follow `Methods`.</summary>
    public static unsafe class Host
    {
        static readonly string[] Methods =
            { "Awake", "OnEnable", "Start", "Update", "FixedUpdate", "LateUpdate", "OnDisable", "OnDestroy" };

        sealed class Entry
        {
            public MonoBehaviour script;
            public Action[] calls;
            public string typeName;
        }

        static readonly List<Entry> entries = new List<Entry> { null };
        static readonly Dictionary<string, Type> types = new Dictionary<string, Type>();

        [UnmanagedCallersOnly]
        public static int Init(void* table)
        {
            Native.api = *(NativeApi*)table;
            types.Clear();
            foreach (var t in typeof(Host).Assembly.GetTypes())
            {
                if (t.IsClass && !t.IsAbstract && typeof(MonoBehaviour).IsAssignableFrom(t))
                {
                    // Scripts are looked up by class name, like Unity (file name == class name).
                    types[t.Name] = t;
                }
            }
            return Methods.Length;
        }

        /// <summary>A new instance of class `name` on object `entity`; its handle, or 0 (the
        /// reason is in the Console).</summary>
        [UnmanagedCallersOnly]
        public static int Create(char* name, int len, ulong entity)
        {
            string typeName = new string(name, 0, len);
            try
            {
                if (!types.TryGetValue(typeName, out var type))
                {
                    Native.Log(2, $"{typeName}.cs: no class named {typeName} that derives from MonoBehaviour");
                    return 0;
                }
                var script = (MonoBehaviour)Activator.CreateInstance(type);
                script.entity = entity;
                var calls = new Action[Methods.Length];
                const BindingFlags flags = BindingFlags.Instance | BindingFlags.Public | BindingFlags.NonPublic;
                for (int i = 0; i < Methods.Length; i++)
                {
                    var m = type.GetMethod(Methods[i], flags, null, Type.EmptyTypes, null);
                    if (m != null && m.ReturnType == typeof(void))
                    {
                        calls[i] = (Action)Delegate.CreateDelegate(typeof(Action), script, m);
                    }
                }
                entries.Add(new Entry { script = script, calls = calls, typeName = typeName });
                return entries.Count - 1;
            }
            catch (Exception e)
            {
                Report(typeName, "constructor", e);
                return 0;
            }
        }

        /// <summary>Whether the script defines method `method` (so the engine can skip it).</summary>
        [UnmanagedCallersOnly]
        public static int Has(int handle, int method)
        {
            var e = Lookup(handle);
            return e != null && (uint)method < (uint)e.calls.Length && e.calls[method] != null ? 1 : 0;
        }

        [UnmanagedCallersOnly]
        public static void Call(int handle, int method) => Invoke(handle, method);

        /// <summary>Call `method` on `count` instances in one transition (Update passes).</summary>
        [UnmanagedCallersOnly]
        public static void CallMany(int* handles, int count, int method)
        {
            for (int i = 0; i < count; i++) Invoke(handles[i], method);
        }

        [UnmanagedCallersOnly]
        public static void Release(int handle)
        {
            if (handle > 0 && handle < entries.Count) entries[handle] = null;
        }

        [UnmanagedCallersOnly]
        public static void SetTime(float delta, float time, float fixedDelta, long frame)
        {
            Time.deltaTime = delta;
            Time.time = time;
            Time.fixedDeltaTime = fixedDelta;
            Time.frameCount = frame;
        }

        static Entry Lookup(int handle) => handle > 0 && handle < entries.Count ? entries[handle] : null;

        static void Invoke(int handle, int method)
        {
            var e = Lookup(handle);
            if (e == null || (uint)method >= (uint)e.calls.Length) return;
            var call = e.calls[method];
            if (call == null) return;
            try
            {
                call();
            }
            catch (Exception ex)
            {
                Report(e.typeName, Methods[method], ex);
            }
        }

        static void Report(string typeName, string method, Exception ex)
        {
            if (ex is TargetInvocationException tie && tie.InnerException != null) ex = tie.InnerException;
            Native.Log(2, $"{typeName} ({method}): {ex.GetType().Name}: {ex.Message}\n{ex.StackTrace}");
        }
    }
}
