// Misty Engine C# scripting API (preview).
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
    // ---- Inspector attributes ----------------------------------------------------------------

    /// <summary>Show and save a private field in the Properties panel.</summary>
    [AttributeUsage(AttributeTargets.Field)]
    public sealed class SerializeField : Attribute { }

    /// <summary>Keep a public field out of the Properties panel (it still saves).</summary>
    [AttributeUsage(AttributeTargets.Field)]
    public sealed class HideInInspector : Attribute { }

    /// <summary>A bold heading above the field in the Properties panel.</summary>
    [AttributeUsage(AttributeTargets.Field, AllowMultiple = true)]
    public sealed class HeaderAttribute : Attribute
    {
        public readonly string header;
        public HeaderAttribute(string header) { this.header = header; }
    }

    /// <summary>Hover text of the field's label.</summary>
    [AttributeUsage(AttributeTargets.Field)]
    public sealed class TooltipAttribute : Attribute
    {
        public readonly string tooltip;
        public TooltipAttribute(string tooltip) { this.tooltip = tooltip; }
    }

    /// <summary>Extra room above the field.</summary>
    [AttributeUsage(AttributeTargets.Field, AllowMultiple = true)]
    public sealed class SpaceAttribute : Attribute
    {
        public readonly float height;
        public SpaceAttribute() { height = 8f; }
        public SpaceAttribute(float height) { this.height = height; }
    }

    /// <summary>A slider between min and max for a number field.</summary>
    [AttributeUsage(AttributeTargets.Field)]
    public sealed class RangeAttribute : Attribute
    {
        public readonly float min, max;
        public RangeAttribute(float min, float max) { this.min = min; this.max = max; }
    }

    // ---- math ----------------------------------------------------------------------------------

    /// <summary>A 2D vector, like Unity's Vector2.</summary>
    [StructLayout(LayoutKind.Sequential)]
    public struct Vector2 : IEquatable<Vector2>
    {
        public float x, y;

        public Vector2(float x, float y) { this.x = x; this.y = y; }

        public static Vector2 zero => new Vector2(0, 0);
        public static Vector2 one => new Vector2(1, 1);
        public static Vector2 up => new Vector2(0, 1);
        public static Vector2 down => new Vector2(0, -1);
        public static Vector2 right => new Vector2(1, 0);
        public static Vector2 left => new Vector2(-1, 0);

        public float magnitude => MathF.Sqrt(x * x + y * y);
        public float sqrMagnitude => x * x + y * y;
        public Vector2 normalized { get { float m = magnitude; return m > 1e-6f ? this / m : zero; } }

        public static Vector2 operator +(Vector2 a, Vector2 b) => new Vector2(a.x + b.x, a.y + b.y);
        public static Vector2 operator -(Vector2 a, Vector2 b) => new Vector2(a.x - b.x, a.y - b.y);
        public static Vector2 operator -(Vector2 a) => new Vector2(-a.x, -a.y);
        public static Vector2 operator *(Vector2 a, float d) => new Vector2(a.x * d, a.y * d);
        public static Vector2 operator *(float d, Vector2 a) => new Vector2(a.x * d, a.y * d);
        public static Vector2 operator /(Vector2 a, float d) => new Vector2(a.x / d, a.y / d);
        public static bool operator ==(Vector2 a, Vector2 b) => (a - b).sqrMagnitude < 1e-10f;
        public static bool operator !=(Vector2 a, Vector2 b) => !(a == b);
        public static implicit operator Vector3(Vector2 v) => new Vector3(v.x, v.y, 0);
        public static implicit operator Vector2(Vector3 v) => new Vector2(v.x, v.y);

        public static float Dot(Vector2 a, Vector2 b) => a.x * b.x + a.y * b.y;
        public static float Distance(Vector2 a, Vector2 b) => (a - b).magnitude;
        public static Vector2 Lerp(Vector2 a, Vector2 b, float t) => a + (b - a) * Mathf.Clamp01(t);

        public bool Equals(Vector2 other) => this == other;
        public override bool Equals(object obj) => obj is Vector2 v && this == v;
        public override int GetHashCode() => HashCode.Combine(x, y);
        public override string ToString() => $"({x:F2}, {y:F2})";
    }

    /// <summary>A position, direction or scale, like Unity's Vector3.</summary>
    [StructLayout(LayoutKind.Sequential)]
    public struct Vector3 : IEquatable<Vector3>
    {
        public float x, y, z;

        public Vector3(float x, float y, float z) { this.x = x; this.y = y; this.z = z; }
        public Vector3(float x, float y) { this.x = x; this.y = y; z = 0; }

        public static Vector3 zero => new Vector3(0, 0, 0);
        public static Vector3 one => new Vector3(1, 1, 1);
        public static Vector3 up => new Vector3(0, 1, 0);
        public static Vector3 down => new Vector3(0, -1, 0);
        public static Vector3 right => new Vector3(1, 0, 0);
        public static Vector3 left => new Vector3(-1, 0, 0);
        public static Vector3 forward => new Vector3(0, 0, 1);
        public static Vector3 back => new Vector3(0, 0, -1);

        public float this[int i]
        {
            get => i == 0 ? x : i == 1 ? y : i == 2 ? z : throw new IndexOutOfRangeException("Vector3 index");
            set { if (i == 0) x = value; else if (i == 1) y = value; else if (i == 2) z = value; else throw new IndexOutOfRangeException("Vector3 index"); }
        }

        public float magnitude => MathF.Sqrt(x * x + y * y + z * z);
        public float sqrMagnitude => x * x + y * y + z * z;
        public Vector3 normalized
        {
            get { float m = magnitude; return m > 1e-6f ? this / m : zero; }
        }
        public void Normalize() => this = normalized;
        public void Set(float x, float y, float z) { this.x = x; this.y = y; this.z = z; }

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
        public static Vector3 Lerp(Vector3 a, Vector3 b, float t) => a + (b - a) * Mathf.Clamp01(t);
        public static Vector3 LerpUnclamped(Vector3 a, Vector3 b, float t) => a + (b - a) * t;
        public static Vector3 Scale(Vector3 a, Vector3 b) => new Vector3(a.x * b.x, a.y * b.y, a.z * b.z);
        public static Vector3 Min(Vector3 a, Vector3 b) => new Vector3(MathF.Min(a.x, b.x), MathF.Min(a.y, b.y), MathF.Min(a.z, b.z));
        public static Vector3 Max(Vector3 a, Vector3 b) => new Vector3(MathF.Max(a.x, b.x), MathF.Max(a.y, b.y), MathF.Max(a.z, b.z));
        public static Vector3 Project(Vector3 v, Vector3 onNormal)
        {
            float d = Dot(onNormal, onNormal);
            return d < 1e-12f ? zero : onNormal * (Dot(v, onNormal) / d);
        }
        public static Vector3 ProjectOnPlane(Vector3 v, Vector3 planeNormal) => v - Project(v, planeNormal);
        public static Vector3 Reflect(Vector3 dir, Vector3 normal) => dir - 2f * Dot(dir, normal) * normal;
        public static Vector3 ClampMagnitude(Vector3 v, float maxLength)
        {
            float m = v.magnitude;
            return m > maxLength && m > 1e-6f ? v * (maxLength / m) : v;
        }
        /// <summary>Angle in degrees between two directions.</summary>
        public static float Angle(Vector3 a, Vector3 b)
        {
            float d = MathF.Sqrt(a.sqrMagnitude * b.sqrMagnitude);
            if (d < 1e-15f) return 0f;
            return MathF.Acos(Mathf.Clamp(Dot(a, b) / d, -1f, 1f)) * Mathf.Rad2Deg;
        }
        public static Vector3 MoveTowards(Vector3 current, Vector3 target, float maxDistanceDelta)
        {
            var delta = target - current;
            float dist = delta.magnitude;
            if (dist <= maxDistanceDelta || dist < 1e-6f) return target;
            return current + delta / dist * maxDistanceDelta;
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

        public Quaternion normalized
        {
            get
            {
                float m = MathF.Sqrt(x * x + y * y + z * z + w * w);
                return m > 1e-9f ? new Quaternion(x / m, y / m, z / m, w / m) : identity;
            }
        }

        /// <summary>Rotation by `angle` degrees around `axis`.</summary>
        public static Quaternion AngleAxis(float angle, Vector3 axis)
        {
            var n = axis.normalized;
            float h = angle * Mathf.Deg2Rad * 0.5f;
            float s = MathF.Sin(h);
            return new Quaternion(n.x * s, n.y * s, n.z * s, MathF.Cos(h));
        }

        public static Quaternion Inverse(Quaternion q)
        {
            float n = q.x * q.x + q.y * q.y + q.z * q.z + q.w * q.w;
            return n < 1e-12f ? identity : new Quaternion(-q.x / n, -q.y / n, -q.z / n, q.w / n);
        }

        public static float Dot(Quaternion a, Quaternion b) => a.x * b.x + a.y * b.y + a.z * b.z + a.w * b.w;

        /// <summary>Angle in degrees between two rotations.</summary>
        public static float Angle(Quaternion a, Quaternion b)
        {
            float d = MathF.Min(MathF.Abs(Dot(a.normalized, b.normalized)), 1f);
            return d > 1f - 1e-6f ? 0f : MathF.Acos(d) * 2f * Mathf.Rad2Deg;
        }

        /// <summary>The rotation that looks along `forward` (+Z) with `up` as up.</summary>
        public static Quaternion LookRotation(Vector3 forward, Vector3 up)
        {
            var f = forward.normalized;
            if (f.sqrMagnitude < 1e-12f) return identity;
            var r = Vector3.Cross(up, f);
            if (r.sqrMagnitude < 1e-12f) r = Vector3.Cross(MathF.Abs(f.y) < 0.999f ? Vector3.up : Vector3.forward, f);
            r = r.normalized;
            var u = Vector3.Cross(f, r);
            // Columns r, u, f of the rotation matrix → quaternion.
            float m00 = r.x, m01 = u.x, m02 = f.x;
            float m10 = r.y, m11 = u.y, m12 = f.y;
            float m20 = r.z, m21 = u.z, m22 = f.z;
            float trace = m00 + m11 + m22;
            Quaternion q;
            if (trace > 0f)
            {
                float s = MathF.Sqrt(trace + 1f) * 2f;
                q = new Quaternion((m21 - m12) / s, (m02 - m20) / s, (m10 - m01) / s, 0.25f * s);
            }
            else if (m00 > m11 && m00 > m22)
            {
                float s = MathF.Sqrt(1f + m00 - m11 - m22) * 2f;
                q = new Quaternion(0.25f * s, (m01 + m10) / s, (m02 + m20) / s, (m21 - m12) / s);
            }
            else if (m11 > m22)
            {
                float s = MathF.Sqrt(1f + m11 - m00 - m22) * 2f;
                q = new Quaternion((m01 + m10) / s, 0.25f * s, (m12 + m21) / s, (m02 - m20) / s);
            }
            else
            {
                float s = MathF.Sqrt(1f + m22 - m00 - m11) * 2f;
                q = new Quaternion((m02 + m20) / s, (m12 + m21) / s, 0.25f * s, (m10 - m01) / s);
            }
            return q.normalized;
        }

        public static Quaternion LookRotation(Vector3 forward) => LookRotation(forward, Vector3.up);

        public static Quaternion Slerp(Quaternion a, Quaternion b, float t) => SlerpUnclamped(a, b, Mathf.Clamp01(t));

        public static Quaternion SlerpUnclamped(Quaternion a, Quaternion b, float t)
        {
            float d = Dot(a, b);
            if (d < 0f) { b = new Quaternion(-b.x, -b.y, -b.z, -b.w); d = -d; }
            if (d > 0.9995f) return Lerp(a, b, t);
            float theta = MathF.Acos(d);
            float s = MathF.Sin(theta);
            float wa = MathF.Sin((1f - t) * theta) / s, wb = MathF.Sin(t * theta) / s;
            return new Quaternion(a.x * wa + b.x * wb, a.y * wa + b.y * wb, a.z * wa + b.z * wb, a.w * wa + b.w * wb);
        }

        public static Quaternion Lerp(Quaternion a, Quaternion b, float t)
        {
            t = Mathf.Clamp01(t);
            if (Dot(a, b) < 0f) b = new Quaternion(-b.x, -b.y, -b.z, -b.w);
            return new Quaternion(a.x + (b.x - a.x) * t, a.y + (b.y - a.y) * t, a.z + (b.z - a.z) * t, a.w + (b.w - a.w) * t).normalized;
        }

        public static Quaternion RotateTowards(Quaternion from, Quaternion to, float maxDegreesDelta)
        {
            float angle = Angle(from, to);
            return angle < 1e-4f ? to : SlerpUnclamped(from, to, MathF.Min(1f, maxDegreesDelta / angle));
        }

        public static Quaternion FromToRotation(Vector3 from, Vector3 to)
        {
            var a = from.normalized; var b = to.normalized;
            float d = Vector3.Dot(a, b);
            if (d > 0.999999f) return identity;
            if (d < -0.999999f)
            {
                var axis = Vector3.Cross(Vector3.right, a);
                if (axis.sqrMagnitude < 1e-6f) axis = Vector3.Cross(Vector3.up, a);
                return AngleAxis(180f, axis);
            }
            var c = Vector3.Cross(a, b);
            return new Quaternion(c.x, c.y, c.z, 1f + d).normalized;
        }

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
        public const float Infinity = float.PositiveInfinity;
        public const float NegativeInfinity = float.NegativeInfinity;
        public static readonly float Epsilon = float.Epsilon;

        public static float Clamp(float v, float min, float max) => v < min ? min : (v > max ? max : v);
        public static int Clamp(int v, int min, int max) => v < min ? min : (v > max ? max : v);
        public static float Clamp01(float v) => Clamp(v, 0f, 1f);
        public static float Lerp(float a, float b, float t) => a + (b - a) * Clamp01(t);
        public static float LerpUnclamped(float a, float b, float t) => a + (b - a) * t;
        public static float InverseLerp(float a, float b, float v) => a != b ? Clamp01((v - a) / (b - a)) : 0f;
        public static float Sin(float f) => MathF.Sin(f);
        public static float Cos(float f) => MathF.Cos(f);
        public static float Tan(float f) => MathF.Tan(f);
        public static float Asin(float f) => MathF.Asin(f);
        public static float Acos(float f) => MathF.Acos(f);
        public static float Atan(float f) => MathF.Atan(f);
        public static float Atan2(float y, float x) => MathF.Atan2(y, x);
        public static float Abs(float f) => MathF.Abs(f);
        public static int Abs(int v) => Math.Abs(v);
        public static float Sqrt(float f) => MathF.Sqrt(f);
        public static float Pow(float f, float p) => MathF.Pow(f, p);
        public static float Exp(float p) => MathF.Exp(p);
        public static float Log(float f) => MathF.Log(f);
        public static float Log(float f, float p) => MathF.Log(f, p);
        public static float Log10(float f) => MathF.Log10(f);
        public static float Min(float a, float b) => MathF.Min(a, b);
        public static float Max(float a, float b) => MathF.Max(a, b);
        public static int Min(int a, int b) => Math.Min(a, b);
        public static int Max(int a, int b) => Math.Max(a, b);
        public static float Sign(float f) => f >= 0f ? 1f : -1f;
        public static float Round(float f) => MathF.Round(f);
        public static float Floor(float f) => MathF.Floor(f);
        public static float Ceil(float f) => MathF.Ceiling(f);
        public static int RoundToInt(float f) => (int)MathF.Round(f);
        public static int FloorToInt(float f) => (int)MathF.Floor(f);
        public static int CeilToInt(float f) => (int)MathF.Ceiling(f);
        public static bool Approximately(float a, float b) =>
            MathF.Abs(b - a) < MathF.Max(1e-6f * MathF.Max(MathF.Abs(a), MathF.Abs(b)), float.Epsilon * 8f);
        public static float Repeat(float t, float length) => Clamp(t - MathF.Floor(t / length) * length, 0f, length);
        public static float PingPong(float t, float length) { t = Repeat(t, length * 2f); return length - MathF.Abs(t - length); }
        public static float DeltaAngle(float current, float target)
        {
            float d = Repeat(target - current, 360f);
            return d > 180f ? d - 360f : d;
        }
        public static float LerpAngle(float a, float b, float t) => a + DeltaAngle(a, b) * Clamp01(t);
        public static float MoveTowards(float current, float target, float maxDelta) =>
            MathF.Abs(target - current) <= maxDelta ? target : current + MathF.Sign(target - current) * maxDelta;
        public static float MoveTowardsAngle(float current, float target, float maxDelta) =>
            MoveTowards(current, current + DeltaAngle(current, target), maxDelta);
        public static float SmoothStep(float from, float to, float t)
        {
            t = Clamp01(t);
            t = -2f * t * t * t + 3f * t * t;
            return to * t + from * (1f - t);
        }
        public static float SmoothDamp(float current, float target, ref float currentVelocity, float smoothTime) =>
            SmoothDamp(current, target, ref currentVelocity, smoothTime, Infinity, Time.deltaTime);
        public static float SmoothDamp(float current, float target, ref float currentVelocity, float smoothTime, float maxSpeed, float deltaTime)
        {
            smoothTime = MathF.Max(0.0001f, smoothTime);
            float omega = 2f / smoothTime;
            float x = omega * deltaTime;
            float exp = 1f / (1f + x + 0.48f * x * x + 0.235f * x * x * x);
            float change = current - target;
            float original = target;
            float maxChange = maxSpeed * smoothTime;
            change = Clamp(change, -maxChange, maxChange);
            target = current - change;
            float temp = (currentVelocity + omega * change) * deltaTime;
            currentVelocity = (currentVelocity - omega * temp) * exp;
            float output = target + (change + temp) * exp;
            if (original - current > 0f == output > original)
            {
                output = original;
                currentVelocity = (output - original) / deltaTime;
            }
            return output;
        }
    }

    /// <summary>Random numbers, like Unity's Random class.</summary>
    public static class Random
    {
        static System.Random rng = new System.Random();

        public static void InitState(int seed) => rng = new System.Random(seed);
        /// <summary>0..1, both included.</summary>
        public static float value => (float)rng.NextDouble();
        /// <summary>min..max, both included.</summary>
        public static float Range(float min, float max) => min + (float)rng.NextDouble() * (max - min);
        /// <summary>min..max-1 (max excluded), like Unity.</summary>
        public static int Range(int min, int max) => max <= min ? min : rng.Next(min, max);
        public static Vector3 insideUnitSphere
        {
            get
            {
                while (true)
                {
                    var v = new Vector3(Range(-1f, 1f), Range(-1f, 1f), Range(-1f, 1f));
                    if (v.sqrMagnitude <= 1f) return v;
                }
            }
        }
        public static Vector2 insideUnitCircle
        {
            get
            {
                while (true)
                {
                    var v = new Vector2(Range(-1f, 1f), Range(-1f, 1f));
                    if (v.sqrMagnitude <= 1f) return v;
                }
            }
        }
        public static Vector3 onUnitSphere => insideUnitSphere.normalized;
        public static Quaternion rotation => Quaternion.Euler(Range(0f, 360f), Range(0f, 360f), Range(0f, 360f));
    }

    /// <summary>The game clock, like Unity's Time class.</summary>
    public static class Time
    {
        public static float deltaTime { get; internal set; }
        public static float unscaledDeltaTime { get; internal set; }
        public static float time { get; internal set; }
        public static float fixedDeltaTime { get; internal set; } = 0.02f;
        public static float timeScale { get; internal set; } = 1f;
        public static int frameCount { get; internal set; }
    }

    /// <summary>Messages to the editor Console.</summary>
    public static class Debug
    {
        public static void Log(object message) => Internal.Native.Log(0, message?.ToString() ?? "null");
        public static void LogWarning(object message) => Internal.Native.Log(1, message?.ToString() ?? "null");
        public static void LogError(object message) => Internal.Native.Log(2, message?.ToString() ?? "null");
        public static void LogFormat(string format, params object[] args) => Log(string.Format(format, args));
    }

    // ---- input ---------------------------------------------------------------------------------

    /// <summary>Keyboard keys and mouse buttons, like Unity's KeyCode.</summary>
    public enum KeyCode
    {
        None,
        A, B, C, D, E, F, G, H, I, J, K, L, M, N, O, P, Q, R, S, T, U, V, W, X, Y, Z,
        Alpha0, Alpha1, Alpha2, Alpha3, Alpha4, Alpha5, Alpha6, Alpha7, Alpha8, Alpha9,
        F1, F2, F3, F4, F5, F6, F7, F8, F9, F10, F11, F12,
        Space, Return, Escape, Tab, Backspace, Delete, Insert, Home, End, PageUp, PageDown,
        UpArrow, DownArrow, LeftArrow, RightArrow,
        LeftShift, RightShift, LeftControl, RightControl, LeftAlt, RightAlt,
        Minus, Equals, Comma, Period, Slash, Backslash, Semicolon, Quote, LeftBracket, RightBracket, BackQuote,
        Mouse0, Mouse1, Mouse2, Mouse3, Mouse4,
    }

    /// <summary>Keyboard, mouse and the Input Manager's axes and buttons, like Unity's Input
    /// class. Key names (`GetKey("left shift")`) are the Input Manager's.</summary>
    public static class Input
    {
        public static bool GetKey(KeyCode key) => Internal.Native.Key(0, Internal.Native.KeyName(key));
        public static bool GetKeyDown(KeyCode key) => Internal.Native.Key(1, Internal.Native.KeyName(key));
        public static bool GetKeyUp(KeyCode key) => Internal.Native.Key(2, Internal.Native.KeyName(key));
        public static bool GetKey(string name) => Internal.Native.Key(0, name);
        public static bool GetKeyDown(string name) => Internal.Native.Key(1, name);
        public static bool GetKeyUp(string name) => Internal.Native.Key(2, name);
        public static bool GetMouseButton(int button) => Internal.Native.Mouse(0, button);
        public static bool GetMouseButtonDown(int button) => Internal.Native.Mouse(1, button);
        public static bool GetMouseButtonUp(int button) => Internal.Native.Mouse(2, button);
        /// <summary>A smoothed Input Manager axis ("Horizontal", "Vertical", "Mouse X"…), -1..1.</summary>
        public static float GetAxis(string axisName) => Internal.Native.Axis(0, axisName);
        public static float GetAxisRaw(string axisName) => Internal.Native.Axis(1, axisName);
        /// <summary>An Input Manager button ("Jump", "Fire1"…) is held.</summary>
        public static bool GetButton(string buttonName) => Internal.Native.Button(0, buttonName);
        public static bool GetButtonDown(string buttonName) => Internal.Native.Button(1, buttonName);
        public static bool GetButtonUp(string buttonName) => Internal.Native.Button(2, buttonName);
        /// <summary>Pointer position in pixels, (0, 0) at the bottom left of the game view.</summary>
        public static Vector3 mousePosition => Internal.Native.InputVector(0);
        public static Vector2 mouseScrollDelta => Internal.Native.InputVector(1);
        /// <summary>Pointer movement this frame in pixels, y up.</summary>
        public static Vector2 mouseDelta => Internal.Native.InputVector(2);
        public static bool anyKey => Internal.Native.InputFlag(0);
        public static bool anyKeyDown => Internal.Native.InputFlag(1);
    }

    // ---- objects -------------------------------------------------------------------------------

    /// <summary>Base of everything in the scene, like Unity's Object. A destroyed object
    /// compares equal to null.</summary>
    public abstract class Object
    {
        internal abstract ulong ObjectId { get; }

        internal bool Alive => Internal.Native.Alive(ObjectId);

        public virtual string name
        {
            get => Internal.Native.GetString(ObjectId, 0);
            set => Internal.Native.SetString(ObjectId, 0, value);
        }

        public static bool operator ==(Object a, Object b)
        {
            bool aNull = a is null || !a.Alive, bNull = b is null || !b.Alive;
            if (aNull || bNull) return aNull && bNull;
            return a.Same(b);
        }

        public static bool operator !=(Object a, Object b) => !(a == b);
        public static implicit operator bool(Object o) => !(o is null) && o.Alive;

        internal virtual bool Same(Object other) => ReferenceEquals(this, other);
        public override bool Equals(object obj) => obj is Object o && this == o;
        public override int GetHashCode() => ObjectId.GetHashCode();
        public override string ToString() => Alive ? name : "null";

        /// <summary>A copy of `original` (with its children and scripts), named "Name(Clone)".</summary>
        public static T Instantiate<T>(T original) where T : Object =>
            Internal.Native.Clone(original, null, null, null);
        public static T Instantiate<T>(T original, Vector3 position, Quaternion rotation) where T : Object =>
            Internal.Native.Clone(original, position, rotation, null);
        public static T Instantiate<T>(T original, Vector3 position, Quaternion rotation, Transform parent) where T : Object =>
            Internal.Native.Clone(original, position, rotation, parent);
        public static T Instantiate<T>(T original, Transform parent) where T : Object =>
            Internal.Native.Clone(original, null, null, parent);

        /// <summary>Destroy a GameObject (and its children) after this frame's updates, or after
        /// `t` seconds.</summary>
        public static void Destroy(Object obj, float t = 0f) => Internal.Host.Destroy(obj, t);

        /// <summary>The first script of type T in the scene (active objects first), or null.</summary>
        public static T FindObjectOfType<T>() where T : MonoBehaviour => Internal.Host.FindOfType<T>();
        public static T[] FindObjectsOfType<T>() where T : MonoBehaviour => Internal.Host.FindAllOfType<T>();
    }

    /// <summary>An object in the scene.</summary>
    public sealed class GameObject : Object
    {
        internal readonly ulong id;
        internal GameObject(ulong id) { this.id = id; }
        internal override ulong ObjectId => id;
        internal override bool Same(Object other) => other is GameObject g && g.id == id;

        public Transform transform => new Transform(id);
        public GameObject gameObject => this;
        public string tag
        {
            get => Internal.Native.GetString(id, 1);
            set => Internal.Native.SetString(id, 1, value);
        }
        public bool CompareTag(string tag) => this.tag == tag;
        public bool activeSelf => Internal.Native.Flag(id, 0);
        public bool activeInHierarchy => Internal.Native.Flag(id, 1);
        public void SetActive(bool value) => Internal.Native.SetActive(id, value);

        /// <summary>The first script of type T on this object (Transform works too), or null.</summary>
        public T GetComponent<T>() where T : class => Internal.Host.GetComponent<T>(id);
        public T[] GetComponents<T>() where T : class => Internal.Host.GetComponents<T>(id);
        public T GetComponentInChildren<T>() where T : class => Internal.Host.GetComponentInChildren<T>(id);
        public T GetComponentInParent<T>() where T : class => Internal.Host.GetComponentInParent<T>(id);
        public bool TryGetComponent<T>(out T component) where T : class
        {
            component = GetComponent<T>();
            return component != null;
        }
        /// <summary>A script by class name, or null.</summary>
        public MonoBehaviour GetComponent(string type) => Internal.Host.GetComponentByName(id, type);

        /// <summary>An object by name anywhere in the open scenes, or null.</summary>
        public static GameObject Find(string name) => Internal.Native.Wrap(Internal.Native.Find(0, name));
        /// <summary>The first active object with `tag`, or null.</summary>
        public static GameObject FindWithTag(string tag) => Internal.Native.Wrap(Internal.Native.Find(1, tag));
        public static GameObject FindGameObjectWithTag(string tag) => FindWithTag(tag);
        /// <summary>Every active object with `tag`.</summary>
        public static GameObject[] FindGameObjectsWithTag(string tag) => Internal.Native.FindAllWithTag(tag);
    }

    /// <summary>Something attached to a GameObject (Transform, scripts).</summary>
    public abstract class Component : Object
    {
        internal abstract ulong EntityId { get; }
        internal override ulong ObjectId => EntityId;
        internal override bool Same(Object other) =>
            other is Component c && c.GetType() == GetType() && c.EntityId == EntityId;

        public GameObject gameObject => new GameObject(EntityId);
        public Transform transform => new Transform(EntityId);
        public override string name { get => gameObject.name; set => gameObject.name = value; }
        public string tag { get => gameObject.tag; set => gameObject.tag = value; }
        public bool CompareTag(string tag) => gameObject.CompareTag(tag);
        public T GetComponent<T>() where T : class => Internal.Host.GetComponent<T>(EntityId);
        public T[] GetComponents<T>() where T : class => Internal.Host.GetComponents<T>(EntityId);
        public T GetComponentInChildren<T>() where T : class => Internal.Host.GetComponentInChildren<T>(EntityId);
        public T GetComponentInParent<T>() where T : class => Internal.Host.GetComponentInParent<T>(EntityId);
        public bool TryGetComponent<T>(out T component) where T : class
        {
            component = GetComponent<T>();
            return component != null;
        }
        public MonoBehaviour GetComponent(string type) => Internal.Host.GetComponentByName(EntityId, type);
    }

    /// <summary>Position, rotation and scale of an object, and its place in the hierarchy.</summary>
    public sealed class Transform : Component
    {
        internal readonly ulong id;
        internal Transform(ulong id) { this.id = id; }
        internal override ulong EntityId => id;
        internal override bool Same(Object other) => other is Transform t && t.id == id;

        // Local values (relative to the parent).
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

        public Vector3 localEulerAngles
        {
            get => localRotation.eulerAngles;
            set => localRotation = Quaternion.Euler(value);
        }

        // World values.
        public Vector3 position
        {
            get => Internal.Native.GetWorld(id).position;
            set => Internal.Native.SetWorldPosition(id, value);
        }

        public Quaternion rotation
        {
            get => Internal.Native.GetWorld(id).rotation;
            set => Internal.Native.SetWorldRotation(id, value);
        }

        /// <summary>The world scale (approximate under rotated, non-uniformly scaled parents).</summary>
        public Vector3 lossyScale => Internal.Native.GetWorld(id).scale;

        public Vector3 eulerAngles { get => rotation.eulerAngles; set => rotation = Quaternion.Euler(value); }

        public Vector3 forward { get => rotation * Vector3.forward; set => rotation = Quaternion.LookRotation(value); }
        public Vector3 right => rotation * Vector3.right;
        public Vector3 up => rotation * Vector3.up;

        public Transform parent
        {
            get { ulong p = Internal.Native.Relative(id, 0, 0); return p == 0 ? null : new Transform(p); }
            set => SetParent(value, true);
        }

        public Transform root
        {
            get { var t = this; while (t.parent is Transform p) t = p; return t; }
        }

        /// <summary>Move under `parent` (null = the scene root); with `worldPositionStays` the
        /// object keeps its place in the world.</summary>
        public void SetParent(Transform parent, bool worldPositionStays = true) =>
            Internal.Native.SetParent(id, parent is null ? 0 : parent.id, worldPositionStays);

        public int childCount => (int)Internal.Native.Relative(id, 2, 0);

        public Transform GetChild(int index)
        {
            ulong c = Internal.Native.Relative(id, 1, index);
            if (c == 0) throw new IndexOutOfRangeException($"Transform.GetChild: no child {index}");
            return new Transform(c);
        }

        /// <summary>A child by name, or a descendant by a slash-separated path; null when missing.</summary>
        public Transform Find(string path)
        {
            ulong c = Internal.Native.FindChild(id, path);
            return c == 0 ? null : new Transform(c);
        }

        public bool IsChildOf(Transform parent)
        {
            for (var t = this; t != null; t = t.parent) if (t.id == parent.id) return true;
            return false;
        }

        /// <summary>Rotate by Euler angles in degrees around the object's own axes.</summary>
        public void Rotate(Vector3 eulers)
        {
            var t = Internal.Native.Get(id);
            t.rotation = t.rotation * Quaternion.Euler(eulers);
            Internal.Native.Set(id, t);
        }

        public void Rotate(float x, float y, float z) => Rotate(new Vector3(x, y, z));

        /// <summary>Rotate by `angle` degrees around `axis` (in the object's own axes).</summary>
        public void Rotate(Vector3 axis, float angle)
        {
            var t = Internal.Native.Get(id);
            t.rotation = t.rotation * Quaternion.AngleAxis(angle, axis);
            Internal.Native.Set(id, t);
        }

        /// <summary>Rotate around a world `point` and `axis` by `angle` degrees.</summary>
        public void RotateAround(Vector3 point, Vector3 axis, float angle)
        {
            var q = Quaternion.AngleAxis(angle, axis);
            position = point + q * (position - point);
            rotation = q * rotation;
        }

        /// <summary>Move by `translation` along the object's own axes.</summary>
        public void Translate(Vector3 translation) => position = position + rotation * translation;

        public void Translate(float x, float y, float z) => Translate(new Vector3(x, y, z));

        /// <summary>Turn so forward (+Z) points at `worldPosition`.</summary>
        public void LookAt(Vector3 worldPosition) => Internal.Native.LookAt(id, worldPosition);
        public void LookAt(Transform target) => LookAt(target.position);

        public Vector3 TransformPoint(Vector3 p) => position + rotation * Vector3.Scale(p, lossyScale);
        public Vector3 TransformDirection(Vector3 d) => rotation * d;
        public Vector3 InverseTransformDirection(Vector3 d) => Quaternion.Inverse(rotation) * d;
        public Vector3 InverseTransformPoint(Vector3 p)
        {
            var s = lossyScale;
            var local = Quaternion.Inverse(rotation) * (p - position);
            return new Vector3(s.x != 0 ? local.x / s.x : 0, s.y != 0 ? local.y / s.y : 0, s.z != 0 ? local.z / s.z : 0);
        }
    }

    // ---- physics ---------------------------------------------------------------------------

    public enum ForceMode { Force, Impulse, VelocityChange, Acceleration }

    /// <summary>A physics body (the object's Rigidbody component).</summary>
    public sealed unsafe class Rigidbody : Component
    {
        internal readonly ulong id;
        internal Rigidbody(ulong id) { this.id = id; }
        internal override ulong EntityId => id;

        Vector3 Vec(int op)
        {
            Vector3 v = default;
            if (Internal.Native.api.rigidbodyVec(id, op, &v, 0) == 0) throw Internal.Native.Missing("Rigidbody");
            return v;
        }

        void SetVec(int op, Vector3 v, ForceMode mode = ForceMode.Force)
        {
            if (Internal.Native.api.rigidbodyVec(id, op, &v, (int)mode) == 0) throw Internal.Native.Missing("Rigidbody");
        }

        float Scalar(int which)
        {
            float v;
            if (Internal.Native.api.rigidbodyScalar(id, which, 0, 0, &v) == 0) throw Internal.Native.Missing("Rigidbody");
            return v;
        }

        void SetScalar(int which, float v)
        {
            if (Internal.Native.api.rigidbodyScalar(id, which, 1, v, null) == 0) throw Internal.Native.Missing("Rigidbody");
        }

        public Vector3 velocity { get => Vec(0); set => SetVec(1, value); }
        /// <summary>Radians per second around each axis.</summary>
        public Vector3 angularVelocity { get => Vec(2); set => SetVec(3, value); }
        public float mass { get => Scalar(0); set => SetScalar(0, value); }
        public bool isKinematic { get => Scalar(1) != 0; set => SetScalar(1, value ? 1 : 0); }
        public bool useGravity { get => Scalar(2) != 0; set => SetScalar(2, value ? 1 : 0); }
        /// <summary>The body's position in the physics world (the Transform may be interpolated).</summary>
        public Vector3 position => Vec(7);
        public Quaternion rotation => transform.rotation;

        public void AddForce(Vector3 force, ForceMode mode = ForceMode.Force) => SetVec(4, force, mode);
        public void AddForce(float x, float y, float z, ForceMode mode = ForceMode.Force) => AddForce(new Vector3(x, y, z), mode);
        public void AddTorque(Vector3 torque, ForceMode mode = ForceMode.Force) => SetVec(5, torque, mode);
        public void AddRelativeForce(Vector3 force, ForceMode mode = ForceMode.Force) => AddForce(transform.rotation * force, mode);
        /// <summary>Kinematic bodies sweep to `position` in the next step, pushing others.</summary>
        public void MovePosition(Vector3 position) => SetVec(6, position);
        public void MoveRotation(Quaternion rotation)
        {
            if (Internal.Native.api.rigidbodyMoveRotation(id, &rotation) == 0) throw Internal.Native.Missing("Rigidbody");
        }
    }

    /// <summary>The colliders of an object (Box, Sphere, Capsule or Mesh Collider).</summary>
    public sealed class Collider : Component
    {
        internal readonly ulong id;
        internal Collider(ulong id) { this.id = id; }
        internal override ulong EntityId => id;
        /// <summary>The Rigidbody this collider moves with (its own or a parent's), or null.</summary>
        public Rigidbody attachedRigidbody => GetComponentInParent<Rigidbody>();
    }

    /// <summary>What an OnCollisionEnter / OnCollisionExit callback hit.</summary>
    public sealed class Collision
    {
        readonly ulong id;
        internal Collision(ulong id) { this.id = id; }
        public GameObject gameObject => new GameObject(id);
        public Transform transform => new Transform(id);
        public Collider collider => new Collider(id);
        public Rigidbody rigidbody => gameObject.GetComponentInParent<Rigidbody>();
    }

    /// <summary>A ray: an origin and a direction.</summary>
    public struct Ray
    {
        public Vector3 origin;
        public Vector3 direction;
        public Ray(Vector3 origin, Vector3 direction) { this.origin = origin; this.direction = direction.normalized; }
        public Vector3 GetPoint(float distance) => origin + direction * distance;
    }

    /// <summary>One hit of a raycast.</summary>
    public struct RaycastHit
    {
        internal Internal.HitData data;
        public Vector3 point => data.point;
        public Vector3 normal => data.normal;
        public float distance => data.distance;
        public Collider collider => data.entity == 0 ? null : new Collider(data.entity);
        public Transform transform => data.entity == 0 ? null : new Transform(data.entity);
        public Rigidbody rigidbody => data.body == 0 ? null : new Rigidbody(data.body);
    }

    /// <summary>Layers by name: masks for raycasts.</summary>
    public unsafe struct LayerMask
    {
        public int value;
        public static implicit operator int(LayerMask m) => m.value;
        public static implicit operator LayerMask(int v) => new LayerMask { value = v };

        public static int NameToLayer(string name)
        {
            fixed (char* p = name) return Internal.Native.api.layer(0, p, name.Length, null, 0);
        }

        public static string LayerToName(int layer)
        {
            char* buf = stackalloc char[64];
            int n = Internal.Native.api.layer(1, null, layer, buf, 64);
            return n <= 0 ? "" : new string(buf, 0, Math.Min(n, 64));
        }

        public static int GetMask(params string[] names)
        {
            int mask = 0;
            foreach (var name in names)
                fixed (char* p = name) mask |= Internal.Native.api.layer(2, p, name.Length, null, 0);
            return mask;
        }
    }

    /// <summary>Scene queries and gravity, like Unity's Physics class.</summary>
    public static unsafe class Physics
    {
        public const int DefaultRaycastLayers = ~(1 << 2);
        public const int AllLayers = ~0;
        public const int IgnoreRaycastLayer = 1 << 2;

        public static Vector3 gravity
        {
            get { Vector3 v; Internal.Native.api.gravity(0, &v); return v; }
            set => Internal.Native.api.gravity(1, &value);
        }

        static bool Cast(Vector3 origin, Vector3 direction, float radius, out RaycastHit hit, float maxDistance, int layerMask)
        {
            Internal.HitData data;
            int r = Internal.Native.api.raycast(&origin, &direction, maxDistance, (uint)layerMask, radius, &data);
            hit = new RaycastHit { data = r == 1 ? data : default };
            return r == 1;
        }

        public static bool Raycast(Vector3 origin, Vector3 direction, float maxDistance = Mathf.Infinity, int layerMask = DefaultRaycastLayers) =>
            Cast(origin, direction, 0f, out _, maxDistance, layerMask);
        public static bool Raycast(Vector3 origin, Vector3 direction, out RaycastHit hitInfo, float maxDistance = Mathf.Infinity, int layerMask = DefaultRaycastLayers) =>
            Cast(origin, direction, 0f, out hitInfo, maxDistance, layerMask);
        public static bool Raycast(Ray ray, float maxDistance = Mathf.Infinity, int layerMask = DefaultRaycastLayers) =>
            Cast(ray.origin, ray.direction, 0f, out _, maxDistance, layerMask);
        public static bool Raycast(Ray ray, out RaycastHit hitInfo, float maxDistance = Mathf.Infinity, int layerMask = DefaultRaycastLayers) =>
            Cast(ray.origin, ray.direction, 0f, out hitInfo, maxDistance, layerMask);
        public static bool SphereCast(Vector3 origin, float radius, Vector3 direction, out RaycastHit hitInfo, float maxDistance = Mathf.Infinity, int layerMask = DefaultRaycastLayers) =>
            Cast(origin, direction, MathF.Max(radius, 1e-4f), out hitInfo, maxDistance, layerMask);

        /// <summary>Every hit along the ray, nearest first.</summary>
        public static RaycastHit[] RaycastAll(Vector3 origin, Vector3 direction, float maxDistance = Mathf.Infinity, int layerMask = DefaultRaycastLayers)
        {
            var buf = new Internal.HitData[32];
            while (true)
            {
                int n;
                fixed (Internal.HitData* b = buf) n = Internal.Native.api.raycastAll(&origin, &direction, maxDistance, (uint)layerMask, b, buf.Length);
                if (n > buf.Length) { buf = new Internal.HitData[n]; continue; }
                var hits = new RaycastHit[n];
                for (int i = 0; i < n; i++) hits[i] = new RaycastHit { data = buf[i] };
                return hits;
            }
        }

        public static RaycastHit[] RaycastAll(Ray ray, float maxDistance = Mathf.Infinity, int layerMask = DefaultRaycastLayers) =>
            RaycastAll(ray.origin, ray.direction, maxDistance, layerMask);

        /// <summary>The colliders touching a sphere.</summary>
        public static Collider[] OverlapSphere(Vector3 position, float radius, int layerMask = AllLayers)
        {
            var buf = new ulong[64];
            while (true)
            {
                int n;
                fixed (ulong* b = buf) n = Internal.Native.api.overlapSphere(&position, radius, (uint)layerMask, b, buf.Length);
                if (n > buf.Length) { buf = new ulong[n]; continue; }
                var found = new Collider[n];
                for (int i = 0; i < n; i++) found[i] = new Collider(buf[i]);
                return found;
            }
        }
    }

    // ---- audio and animation -----------------------------------------------------------------

    /// <summary>The object's Audio Source: plays its clip.</summary>
    public sealed unsafe class AudioSource : Component
    {
        internal readonly ulong id;
        internal AudioSource(ulong id) { this.id = id; }
        internal override ulong EntityId => id;

        int Op(int op)
        {
            int r = Internal.Native.api.audio(id, op);
            if (r < 0) throw Internal.Native.Missing("AudioSource");
            return r;
        }

        float Get(int which)
        {
            float v;
            if (Internal.Native.api.audioScalar(id, which, 0, 0, &v) == 0) throw Internal.Native.Missing("AudioSource");
            return v;
        }

        void Set(int which, float v)
        {
            if (Internal.Native.api.audioScalar(id, which, 1, v, null) == 0) throw Internal.Native.Missing("AudioSource");
        }

        public void Play() => Op(0);
        public void Stop() => Op(1);
        public void Pause() => Op(2);
        public void UnPause() => Op(3);
        public bool isPlaying => Op(4) == 1;
        public float volume { get => Get(0); set => Set(0, value); }
        public float pitch { get => Get(1); set => Set(1, value); }
        public bool loop { get => Get(2) != 0; set => Set(2, value ? 1 : 0); }
        public bool mute { get => Get(3) != 0; set => Set(3, value ? 1 : 0); }
        /// <summary>Playback position in seconds.</summary>
        public float time { get => Get(4); set => Set(4, value); }
    }

    /// <summary>The object's Animator: parameters and states of its controller.</summary>
    public sealed unsafe class Animator : Component
    {
        internal readonly ulong id;
        internal Animator(ulong id) { this.id = id; }
        internal override ulong EntityId => id;

        float Param(int kind, string name, bool set, float value)
        {
            float v = 0;
            int r;
            fixed (char* p = name) r = Internal.Native.api.animatorParam(id, kind, p, name.Length, set ? 1 : 0, value, &v);
            if (r == 0) throw Internal.Native.Missing("Animator");
            return v;
        }

        void Play(int op, string state, int layer, float a, float b)
        {
            int r;
            fixed (char* p = state) r = Internal.Native.api.animatorPlay(id, op, p, state.Length, layer, a, b);
            if (r == 0 && !Alive) throw Internal.Native.Missing("Animator");
        }

        public void SetFloat(string name, float value) => Param(0, name, true, value);
        public float GetFloat(string name) => Param(0, name, false, 0);
        public void SetInteger(string name, int value) => Param(1, name, true, value);
        public int GetInteger(string name) => (int)MathF.Round(Param(1, name, false, 0));
        public void SetBool(string name, bool value) => Param(2, name, true, value ? 1 : 0);
        public bool GetBool(string name) => Param(2, name, false, 0) >= 0.5f;
        public void SetTrigger(string name) => Param(3, name, true, 1);
        public void ResetTrigger(string name) => Param(3, name, true, 0);
        /// <summary>Jump to `stateName` (layer -1 searches every layer).</summary>
        public void Play(string stateName, int layer = -1, float normalizedTime = float.NaN) =>
            Play(0, stateName, layer, normalizedTime, 0);
        public void CrossFade(string stateName, float normalizedTransitionDuration, int layer = -1, float normalizedTimeOffset = 0f) =>
            Play(1, stateName, layer, normalizedTransitionDuration, normalizedTimeOffset);
        public void CrossFadeInFixedTime(string stateName, float fixedTransitionDuration, int layer = -1, float normalizedTimeOffset = 0f) =>
            Play(2, stateName, layer, fixedTransitionDuration, normalizedTimeOffset);
    }

    // ---- colour, cameras, lights, renderers and materials ------------------------------------

    /// <summary>A colour: red, green, blue and alpha, each 0..1 (HDR colours go above 1).</summary>
    [StructLayout(LayoutKind.Sequential)]
    public struct Color : IEquatable<Color>
    {
        public float r, g, b, a;
        public Color(float r, float g, float b, float a = 1f) { this.r = r; this.g = g; this.b = b; this.a = a; }

        public static Color white => new Color(1, 1, 1);
        public static Color black => new Color(0, 0, 0);
        public static Color red => new Color(1, 0, 0);
        public static Color green => new Color(0, 1, 0);
        public static Color blue => new Color(0, 0, 1);
        public static Color yellow => new Color(1, 0.92f, 0.016f);
        public static Color cyan => new Color(0, 1, 1);
        public static Color magenta => new Color(1, 0, 1);
        public static Color gray => new Color(0.5f, 0.5f, 0.5f);
        public static Color grey => gray;
        public static Color clear => new Color(0, 0, 0, 0);

        /// <summary>Perceived brightness (0..1 for colours in range).</summary>
        public float grayscale => 0.299f * r + 0.587f * g + 0.114f * b;
        public float maxColorComponent => Mathf.Max(r, Mathf.Max(g, b));

        public static Color Lerp(Color a, Color b, float t) => LerpUnclamped(a, b, Mathf.Clamp01(t));
        public static Color LerpUnclamped(Color a, Color b, float t) =>
            new Color(a.r + (b.r - a.r) * t, a.g + (b.g - a.g) * t, a.b + (b.b - a.b) * t, a.a + (b.a - a.a) * t);

        /// <summary>A colour from hue, saturation and value, each 0..1.</summary>
        public static Color HSVToRGB(float h, float s, float v)
        {
            h = Mathf.Repeat(h, 1f) * 6f;
            int i = (int)Mathf.Floor(h);
            float f = h - i, p = v * (1 - s), q = v * (1 - s * f), t = v * (1 - s * (1 - f));
            return i switch
            {
                0 => new Color(v, t, p),
                1 => new Color(q, v, p),
                2 => new Color(p, v, t),
                3 => new Color(p, q, v),
                4 => new Color(t, p, v),
                _ => new Color(v, p, q),
            };
        }

        public static Color operator +(Color a, Color b) => new Color(a.r + b.r, a.g + b.g, a.b + b.b, a.a + b.a);
        public static Color operator -(Color a, Color b) => new Color(a.r - b.r, a.g - b.g, a.b - b.b, a.a - b.a);
        public static Color operator *(Color a, Color b) => new Color(a.r * b.r, a.g * b.g, a.b * b.b, a.a * b.a);
        public static Color operator *(Color a, float k) => new Color(a.r * k, a.g * k, a.b * k, a.a * k);
        public static Color operator *(float k, Color a) => a * k;
        public static Color operator /(Color a, float k) => new Color(a.r / k, a.g / k, a.b / k, a.a / k);
        public static bool operator ==(Color a, Color b) => a.r == b.r && a.g == b.g && a.b == b.b && a.a == b.a;
        public static bool operator !=(Color a, Color b) => !(a == b);
        public bool Equals(Color other) => this == other;
        public override bool Equals(object obj) => obj is Color c && this == c;
        public override int GetHashCode() => HashCode.Combine(r, g, b, a);
        public override string ToString() => $"RGBA({r:F3}, {g:F3}, {b:F3}, {a:F3})";
    }

    /// <summary>A component with an on/off checkbox.</summary>
    public abstract unsafe class Behaviour : Component
    {
        internal readonly ulong id;
        internal Behaviour(ulong id) { this.id = id; }
        internal override ulong EntityId => id;
        internal abstract int Kind { get; }
        internal abstract string TypeName { get; }

        internal Color Get(int which)
        {
            Color v = default;
            if (Internal.Native.api.componentValue(id, Kind, which, 0, &v) == 0) throw Internal.Native.Missing(TypeName);
            return v;
        }

        internal void Set(int which, Color v)
        {
            if (Internal.Native.api.componentValue(id, Kind, which, 1, &v) == 0) throw Internal.Native.Missing(TypeName);
        }

        internal float GetFloat(int which) => Get(which).r;
        internal void SetFloat(int which, float v) => Set(which, new Color(v, 0, 0, 0));
        internal bool GetBool(int which) => Get(which).r != 0;
        internal void SetBool(int which, bool v) => Set(which, new Color(v ? 1 : 0, 0, 0, 0));
    }

    /// <summary>The object's Camera.</summary>
    public sealed unsafe class Camera : Behaviour
    {
        internal Camera(ulong id) : base(id) { }
        internal override int Kind => 4;
        internal override string TypeName => "Camera";

        /// <summary>The first enabled camera tagged MainCamera, or null.</summary>
        public static Camera main
        {
            get
            {
                ulong id = Internal.Native.api.global(0, 0, null);
                return id == 0 ? null : new Camera(id);
            }
        }

        /// <summary>Vertical field of view in degrees.</summary>
        public float fieldOfView { get => GetFloat(0); set => SetFloat(0, value); }
        public float nearClipPlane { get => GetFloat(1); set => SetFloat(1, value); }
        public float farClipPlane { get => GetFloat(2); set => SetFloat(2, value); }
        public bool orthographic { get => GetBool(3); set => SetBool(3, value); }
        /// <summary>Half the height of an orthographic camera's view, in world units.</summary>
        public float orthographicSize { get => GetFloat(4); set => SetFloat(4, value); }
        public bool enabled { get => GetBool(5); set => SetBool(5, value); }
        /// <summary>Layer bits the camera renders (`~(1 &lt;&lt; layer)` hides one).</summary>
        public int cullingMask
        {
            get => BitConverter.SingleToInt32Bits(GetFloat(6));
            set => SetFloat(6, BitConverter.Int32BitsToSingle(value));
        }
        public Color backgroundColor { get => Get(7); set => Set(7, value); }
        /// <summary>Draw order: cameras with a higher depth draw on top.</summary>
        public float depth { get => GetFloat(8); set => SetFloat(8, value); }
        public int pixelWidth => (int)Get(9).r;
        public int pixelHeight => (int)Get(9).g;
        /// <summary>Width divided by height.</summary>
        public float aspect => Get(9).b;

        Vector3 Point(int op, Vector3 p, out Vector3 second)
        {
            Vector3 a, b;
            if (Internal.Native.api.cameraPoint(id, op, &p, &a, &b) == 0) throw Internal.Native.Missing("Camera");
            second = b;
            return a;
        }

        /// <summary>A ray from the camera through a screen point (pixels from the bottom-left,
        /// like Input.mousePosition).</summary>
        public Ray ScreenPointToRay(Vector3 position)
        {
            var origin = Point(0, position, out var direction);
            return new Ray(origin, direction);
        }
        /// <summary>A ray through a viewport point (0..1 from the bottom-left).</summary>
        public Ray ViewportPointToRay(Vector3 position)
        {
            var origin = Point(1, position, out var direction);
            return new Ray(origin, direction);
        }
        /// <summary>World to screen pixels; z is the distance in front of the camera.</summary>
        public Vector3 WorldToScreenPoint(Vector3 position) => Point(2, position, out _);
        public Vector3 WorldToViewportPoint(Vector3 position) => Point(3, position, out _);
        /// <summary>Screen pixels to world; z is the distance in front of the camera.</summary>
        public Vector3 ScreenToWorldPoint(Vector3 position) => Point(4, position, out _);
        public Vector3 ViewportToWorldPoint(Vector3 position) => Point(5, position, out _);
    }

    public enum LightType { Spot, Directional, Point, Rectangle }
    public enum LightShadows { None, Hard, Soft }

    /// <summary>The object's Light.</summary>
    public sealed class Light : Behaviour
    {
        internal Light(ulong id) : base(id) { }
        internal override int Kind => 5;
        internal override string TypeName => "Light";

        public bool enabled { get => GetBool(0); set => SetBool(0, value); }
        public float intensity { get => GetFloat(1); set => SetFloat(1, value); }
        /// <summary>How far a point or spot light reaches, in metres.</summary>
        public float range { get => GetFloat(2); set => SetFloat(2, value); }
        /// <summary>Full cone angle of a spot light in degrees.</summary>
        public float spotAngle { get => GetFloat(3); set => SetFloat(3, value); }
        public Color color { get => Get(4); set => Set(4, value); }
        public LightType type { get => (LightType)(int)GetFloat(5); set => SetFloat(5, (int)value); }
        public LightShadows shadows { get => (LightShadows)(int)GetFloat(6); set => SetFloat(6, (int)value); }
    }

    /// <summary>The object's Mesh Renderer: draws its mesh with its materials.</summary>
    public unsafe class Renderer : Behaviour
    {
        internal Renderer(ulong id) : base(id) { }
        internal override int Kind => 6;
        internal override string TypeName => "MeshRenderer";

        public bool enabled { get => GetBool(0); set => SetBool(0, value); }
        public bool castShadows { get => GetBool(1); set => SetBool(1, value); }
        public bool receiveShadows { get => GetBool(2); set => SetBool(2, value); }
        /// <summary>This renderer's own copy of its first material, made on first use: changes
        /// show on this object only, and last until Play mode ends.</summary>
        public Material material => new Material(id, false);
        /// <summary>The material asset itself: changes show on every object using it, and last
        /// until Play mode ends.</summary>
        public Material sharedMaterial => new Material(id, true);
    }

    public sealed class MeshRenderer : Renderer
    {
        internal MeshRenderer(ulong id) : base(id) { }
    }

    /// <summary>A material of a renderer, with the Standard shader's property names:
    /// `_Color`, `_EmissionColor`, `_Metallic`, `_Glossiness`, `_BumpScale`, `_Cutoff`, …</summary>
    public sealed unsafe class Material
    {
        readonly ulong id;
        readonly bool shared;
        internal Material(ulong id, bool shared) { this.id = id; this.shared = shared; }

        int Call(int op, string name, Color* v)
        {
            name ??= "";
            int r;
            fixed (char* p = name) r = Internal.Native.api.material(id, shared ? 1 : 0, op, p, name.Length, v);
            if (r == -1) throw Internal.Native.Missing("MeshRenderer");
            if (r == -2) throw new InvalidOperationException("Default-Material can't be changed; assign a material first.");
            return r;
        }

        Color Read(int op, string name)
        {
            Color v = default;
            Call(op, name, &v);
            return v;
        }

        public Color color { get => GetColor("_Color"); set => SetColor("_Color", value); }
        public Vector2 mainTextureOffset
        {
            get { var v = Read(8, null); return new Vector2(v.r, v.g); }
            set { var v = new Color(value.x, value.y, 0, 0); Call(9, null, &v); }
        }
        public Vector2 mainTextureScale
        {
            get { var v = Read(10, null); return new Vector2(v.r, v.g); }
            set { var v = new Color(value.x, value.y, 0, 0); Call(11, null, &v); }
        }

        /// <summary>A colour property (black when the material has no such property).</summary>
        public Color GetColor(string name) => Read(0, name);
        public void SetColor(string name, Color value) => Call(1, name, &value);
        /// <summary>A number property (0 when the material has no such property).</summary>
        public float GetFloat(string name) => Read(2, name).r;
        public void SetFloat(string name, float value)
        {
            var v = new Color(value, 0, 0, 0);
            Call(3, name, &v);
        }
        public bool HasProperty(string name) => Call(4, name, null) == 1;
        public bool IsKeywordEnabled(string keyword) => Call(5, keyword, null) == 1;
        /// <summary>`_EMISSION` and the feature keywords are switches; the other keywords follow
        /// the material's settings.</summary>
        public void EnableKeyword(string keyword) => Call(6, keyword, null);
        public void DisableKeyword(string keyword) => Call(7, keyword, null);
    }

    /// <summary>The game image.</summary>
    public static unsafe class Screen
    {
        static Color Size()
        {
            Color v = default;
            Internal.Native.api.global(1, 0, &v);
            return v;
        }

        /// <summary>Width of the game image in pixels.</summary>
        public static int width => (int)Size().r;
        public static int height => (int)Size().g;
    }

    public enum CursorLockMode { None, Locked, Confined }

    /// <summary>The mouse cursor: lock it for mouse look, hide or show it.</summary>
    public static unsafe class Cursor
    {
        static float Get(int which)
        {
            Color v = default;
            Internal.Native.api.global(which, 0, &v);
            return v.r;
        }

        static void Set(int which, float value)
        {
            var v = new Color(value, 0, 0, 0);
            Internal.Native.api.global(which, 1, &v);
        }

        /// <summary>Locked holds the cursor at the centre and hides it; Confined keeps it in
        /// the window.</summary>
        public static CursorLockMode lockState { get => (CursorLockMode)(int)Get(2); set => Set(2, (int)value); }
        public static bool visible { get => Get(3) != 0; set => Set(3, value ? 1 : 0); }
    }

    // ---- scenes and the application ----------------------------------------------------------

    public enum LoadSceneMode { Single, Additive }

    /// <summary>Scene loading (carried out after the frame), like Unity's SceneManager.</summary>
    public static unsafe class SceneManager
    {
        static void Request(int op, int index, string name)
        {
            name ??= "";
            fixed (char* p = name) Internal.Native.api.request(op, index, p, name.Length);
        }

        public static void LoadScene(string sceneName, LoadSceneMode mode = LoadSceneMode.Single) =>
            Request(mode == LoadSceneMode.Additive ? 1 : 0, -1, sceneName);
        public static void LoadScene(int sceneBuildIndex, LoadSceneMode mode = LoadSceneMode.Single) =>
            Request(mode == LoadSceneMode.Additive ? 1 : 0, sceneBuildIndex, null);
        public static void UnloadSceneAsync(string sceneName) => Request(2, -1, sceneName);
        public static void UnloadSceneAsync(int sceneBuildIndex) => Request(2, sceneBuildIndex, null);
    }

    public static unsafe class Application
    {
        /// <summary>Quit the game (stops Play mode in the editor).</summary>
        public static void Quit() => Internal.Native.api.request(3, -1, null, 0);
        public static bool isPlaying => true;
    }

    // ---- coroutines ------------------------------------------------------------------------

    /// <summary>Base of the things a coroutine can `yield return`.</summary>
    public abstract class YieldInstruction { }

    /// <summary>Resume after `seconds` of game time (scaled by Time.timeScale).</summary>
    public sealed class WaitForSeconds : YieldInstruction
    {
        internal readonly float seconds;
        public WaitForSeconds(float seconds) { this.seconds = seconds; }
    }

    /// <summary>Resume after `seconds` of real time.</summary>
    public sealed class WaitForSecondsRealtime : YieldInstruction
    {
        internal readonly float seconds;
        public WaitForSecondsRealtime(float seconds) { this.seconds = seconds; }
    }

    /// <summary>Resume next frame (coroutines run once per frame, after Update).</summary>
    public sealed class WaitForEndOfFrame : YieldInstruction { }

    /// <summary>Resume next frame (coroutines run once per frame, after Update).</summary>
    public sealed class WaitForFixedUpdate : YieldInstruction { }

    /// <summary>Resume once `predicate` is true.</summary>
    public sealed class WaitUntil : YieldInstruction
    {
        internal readonly Func<bool> predicate;
        public WaitUntil(Func<bool> predicate) { this.predicate = predicate; }
    }

    /// <summary>Resume once `predicate` is false.</summary>
    public sealed class WaitWhile : YieldInstruction
    {
        internal readonly Func<bool> predicate;
        public WaitWhile(Func<bool> predicate) { this.predicate = predicate; }
    }

    /// <summary>A custom wait: the coroutine resumes once `keepWaiting` is false.</summary>
    public abstract class CustomYieldInstruction : System.Collections.IEnumerator
    {
        public abstract bool keepWaiting { get; }
        public object Current => null;
        public bool MoveNext() => keepWaiting;
        public virtual void Reset() { }
    }

    /// <summary>A running coroutine (from StartCoroutine); `yield return` it to wait for it.</summary>
    public sealed class Coroutine : YieldInstruction
    {
        internal readonly MonoBehaviour owner;
        internal readonly Stack<System.Collections.IEnumerator> stack = new Stack<System.Collections.IEnumerator>();
        internal object waiting;
        internal float resumeAt;
        internal long resumeFrame;
        /// <summary>It yielded at least once (so its wait applies).</summary>
        internal bool yielded;
        internal bool done;
        internal Coroutine(MonoBehaviour owner, System.Collections.IEnumerator routine) { this.owner = owner; stack.Push(routine); }
    }

    /// <summary>Base class of C# scripts, like Unity's MonoBehaviour. Awake, OnEnable, Start,
    /// Update, FixedUpdate, LateUpdate, OnDisable and OnDestroy are found by name (any
    /// visibility, no parameters). Public fields (and `[SerializeField]` private ones) of type
    /// float, double, int, bool, string and Vector3 show in the Properties panel.</summary>
    public abstract class MonoBehaviour : Component
    {
        internal ulong entity;
        internal int slot;
        internal int handle;
        internal override ulong EntityId => entity;

        /// <summary>The script's checkbox: a disabled script gets no Update calls.</summary>
        public bool enabled
        {
            get => Internal.Native.ScriptEnabled(entity, slot, -1);
            set => Internal.Native.ScriptEnabled(entity, slot, value ? 1 : 0);
        }

        public bool isActiveAndEnabled => gameObject.activeInHierarchy && enabled;

        public static void print(object message) => Debug.Log(message);

        internal override bool Same(Object other) => ReferenceEquals(this, other);

        /// <summary>Run `routine` until its first `yield`, then once per frame after Update
        /// (`yield return null`, `new WaitForSeconds(t)`, `new WaitUntil(…)`, another coroutine…).
        /// It stops when the object is switched off or destroyed.</summary>
        public Coroutine StartCoroutine(System.Collections.IEnumerator routine)
        {
            if (routine == null) throw new ArgumentNullException(nameof(routine));
            return Internal.Coroutines.Start(this, routine);
        }

        public void StopCoroutine(Coroutine routine) => Internal.Coroutines.Stop(routine);
        public void StopAllCoroutines() => Internal.Coroutines.StopAll(this);

        /// <summary>Call method `methodName` (no parameters) after `time` seconds.</summary>
        public void Invoke(string methodName, float time) =>
            Internal.Coroutines.MarkInvoke(StartCoroutine(Internal.Coroutines.Delayed(this, methodName, time, -1f)));

        /// <summary>Call method `methodName` after `time` seconds, then every `repeatRate` seconds.</summary>
        public void InvokeRepeating(string methodName, float time, float repeatRate) =>
            Internal.Coroutines.MarkInvoke(StartCoroutine(Internal.Coroutines.Delayed(this, methodName, time, repeatRate)));

        /// <summary>Stop the pending Invoke / InvokeRepeating calls of this script.</summary>
        public void CancelInvoke() => Internal.Coroutines.StopInvokes(this);
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
        public delegate* unmanaged<ulong, int, char*, int, int> getString;
        public delegate* unmanaged<ulong, int, char*, int, int> setString;
        public delegate* unmanaged<ulong, TransformData*, int> getWorld;
        public delegate* unmanaged<ulong, Vector3*, int> setWorldPosition;
        public delegate* unmanaged<ulong, Quaternion*, int> setWorldRotation;
        public delegate* unmanaged<ulong, int, int> getFlag;
        public delegate* unmanaged<ulong, int, int> setActive;
        public delegate* unmanaged<ulong, int, int, ulong> relative;
        public delegate* unmanaged<ulong, ulong, int, int> setParent;
        public delegate* unmanaged<int, char*, int, ulong> find;
        public delegate* unmanaged<ulong, char*, int, ulong> findChild;
        public delegate* unmanaged<char*, int, ulong*, int, int> findAllWithTag;
        public delegate* unmanaged<ulong, int, Vector3*, Quaternion*, ulong, ulong> instantiate;
        public delegate* unmanaged<ulong, int> destroy;
        public delegate* unmanaged<ulong, int, int, int> scriptEnabled;
        public delegate* unmanaged<int, char*, int, int> inputKey;
        public delegate* unmanaged<int, int, int> inputMouse;
        public delegate* unmanaged<int, char*, int, float*, int> inputAxis;
        public delegate* unmanaged<int, char*, int, int> inputButton;
        public delegate* unmanaged<int, Vector3*, void> inputVector;
        public delegate* unmanaged<int, int> inputFlag;
        public delegate* unmanaged<ulong, Vector3*, int> lookAt;
        public delegate* unmanaged<ulong, int, int> hasComponent;
        public delegate* unmanaged<ulong, int, Vector3*, int, int> rigidbodyVec;
        public delegate* unmanaged<ulong, Quaternion*, int> rigidbodyMoveRotation;
        public delegate* unmanaged<ulong, int, int, float, float*, int> rigidbodyScalar;
        public delegate* unmanaged<Vector3*, Vector3*, float, uint, float, HitData*, int> raycast;
        public delegate* unmanaged<Vector3*, Vector3*, float, uint, HitData*, int, int> raycastAll;
        public delegate* unmanaged<Vector3*, float, uint, ulong*, int, int> overlapSphere;
        public delegate* unmanaged<int, Vector3*, void> gravity;
        public delegate* unmanaged<int, char*, int, char*, int, int> layer;
        public delegate* unmanaged<ulong, int, int> audio;
        public delegate* unmanaged<ulong, int, int, float, float*, int> audioScalar;
        public delegate* unmanaged<ulong, int, char*, int, int, float, float*, int> animatorParam;
        public delegate* unmanaged<ulong, int, char*, int, int, float, float, int> animatorPlay;
        public delegate* unmanaged<int, long, char*, int, void> request;
        public delegate* unmanaged<ulong, int, int, int, Color*, int> componentValue;
        public delegate* unmanaged<ulong, int, Vector3*, Vector3*, Vector3*, int> cameraPoint;
        public delegate* unmanaged<ulong, int, int, char*, int, Color*, int> material;
        public delegate* unmanaged<int, int, Color*, ulong> global;
    }

    /// <summary>Same layout as `HitData` in csharp_native.rs.</summary>
    [StructLayout(LayoutKind.Sequential)]
    internal struct HitData
    {
        public Vector3 point;
        public Vector3 normal;
        public float distance;
        public ulong entity;
        public ulong body;
    }

    internal static unsafe class Native
    {
        internal static NativeApi api;

        static Exception Destroyed() =>
            new InvalidOperationException("The object has been destroyed but you are still trying to access it.");

        /// <summary>A component that is gone (object destroyed or component removed).</summary>
        internal static Exception Missing(string component) =>
            new InvalidOperationException($"There is no '{component}' attached to the object (or it was destroyed).");

        internal static void Log(int kind, string text)
        {
            fixed (char* p = text) api.log(kind, p, text.Length);
        }

        internal static TransformData Get(ulong id)
        {
            TransformData t;
            if (api.getTransform(id, &t) == 0) throw Destroyed();
            return t;
        }

        internal static void Set(ulong id, TransformData t)
        {
            if (api.setTransform(id, &t) == 0) throw Destroyed();
        }

        internal static TransformData GetWorld(ulong id)
        {
            TransformData t;
            if (api.getWorld(id, &t) == 0) throw Destroyed();
            return t;
        }

        internal static void SetWorldPosition(ulong id, Vector3 p)
        {
            if (api.setWorldPosition(id, &p) == 0) throw Destroyed();
        }

        internal static void SetWorldRotation(ulong id, Quaternion q)
        {
            if (api.setWorldRotation(id, &q) == 0) throw Destroyed();
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

        /// <summary>which: 0 name, 1 tag.</summary>
        internal static string GetString(ulong id, int which)
        {
            char* buf = stackalloc char[256];
            int n = api.getString(id, which, buf, 256);
            if (n < 0) throw Destroyed();
            if (n <= 256) return new string(buf, 0, n);
            var big = new char[n];
            fixed (char* p = big)
            {
                n = api.getString(id, which, p, big.Length);
                return new string(p, 0, Math.Min(Math.Max(n, 0), big.Length));
            }
        }

        internal static void SetString(ulong id, int which, string value)
        {
            value ??= "";
            int r;
            fixed (char* p = value) r = api.setString(id, which, p, value.Length);
            if (r < 0) throw Destroyed();
            if (r == 0) throw new ArgumentException($"Tag: {value} is not defined.");
        }

        internal static bool Alive(ulong id) => id != 0 && api.getFlag(id, 2) == 1;

        /// <summary>which: 0 activeSelf, 1 activeInHierarchy.</summary>
        internal static bool Flag(ulong id, int which)
        {
            int r = api.getFlag(id, which);
            if (r < 0) throw Destroyed();
            return r == 1;
        }

        internal static void SetActive(ulong id, bool on)
        {
            if (api.setActive(id, on ? 1 : 0) == 0) throw Destroyed();
        }

        /// <summary>which: 0 parent, 1 child `index`, 2 child count.</summary>
        internal static ulong Relative(ulong id, int which, int index)
        {
            if (!Alive(id)) throw Destroyed();
            return api.relative(id, which, index);
        }

        internal static void SetParent(ulong id, ulong parent, bool keepWorld)
        {
            int r = api.setParent(id, parent, keepWorld ? 1 : 0);
            if (r < 0) throw Destroyed();
            if (r == 0) throw new InvalidOperationException("SetParent: an object can't be moved under itself or its children.");
        }

        internal static ulong Find(int which, string name)
        {
            ulong r;
            fixed (char* p = name) r = api.find(which, p, name.Length);
            if (r == ulong.MaxValue) throw new ArgumentException($"Tag: {name} is not defined.");
            return r;
        }

        internal static GameObject Wrap(ulong id) => id == 0 ? null : new GameObject(id);

        internal static ulong FindChild(ulong id, string path)
        {
            if (!Alive(id)) throw Destroyed();
            fixed (char* p = path) return api.findChild(id, p, path.Length);
        }

        internal static GameObject[] FindAllWithTag(string tag)
        {
            var buf = new ulong[256];
            while (true)
            {
                int n;
                fixed (char* p = tag)
                fixed (ulong* b = buf)
                    n = api.findAllWithTag(p, tag.Length, b, buf.Length);
                if (n < 0) throw new ArgumentException($"Tag: {tag} is not defined.");
                if (n > buf.Length) { buf = new ulong[n]; continue; }
                var result = new GameObject[n];
                for (int i = 0; i < n; i++) result[i] = new GameObject(buf[i]);
                return result;
            }
        }

        internal static T Clone<T>(T original, Vector3? position, Quaternion? rotation, Transform parent) where T : Object
        {
            if (original is null || !original.Alive) throw new ArgumentException("The Object you want to instantiate is null.");
            if (original is MonoBehaviour)
                throw new NotSupportedException(
                    "Instantiate(script) is not supported yet: instantiate its gameObject and call GetComponent on the copy after this callback.");
            Vector3 pos = position ?? default;
            Quaternion rot = rotation ?? Quaternion.identity;
            int flags = (position.HasValue ? 1 : 0) | (rotation.HasValue ? 2 : 0);
            ulong copy = api.instantiate(original.ObjectId, flags, &pos, &rot, parent is null ? 0 : parent.id);
            if (copy == 0) throw new InvalidOperationException("Instantiate failed (see the Console).");
            object result = original is Transform ? new Transform(copy) : new GameObject(copy);
            return (T)result;
        }

        internal static void Destroy(ulong id) => api.destroy(id);

        internal static bool ScriptEnabled(ulong id, int slot, int set)
        {
            int r = api.scriptEnabled(id, slot, set);
            if (r < 0) throw Destroyed();
            return r == 1;
        }

        static readonly Dictionary<KeyCode, string> keyNames = BuildKeyNames();

        static Dictionary<KeyCode, string> BuildKeyNames()
        {
            var d = new Dictionary<KeyCode, string>();
            for (var k = KeyCode.A; k <= KeyCode.Z; k++) d[k] = ((char)('a' + (k - KeyCode.A))).ToString();
            for (var k = KeyCode.Alpha0; k <= KeyCode.Alpha9; k++) d[k] = ((char)('0' + (k - KeyCode.Alpha0))).ToString();
            for (var k = KeyCode.F1; k <= KeyCode.F12; k++) d[k] = "f" + (1 + (k - KeyCode.F1));
            for (var k = KeyCode.Mouse0; k <= KeyCode.Mouse4; k++) d[k] = "mouse " + (k - KeyCode.Mouse0);
            d[KeyCode.Space] = "space"; d[KeyCode.Return] = "return"; d[KeyCode.Escape] = "escape";
            d[KeyCode.Tab] = "tab"; d[KeyCode.Backspace] = "backspace"; d[KeyCode.Delete] = "delete";
            d[KeyCode.Insert] = "insert"; d[KeyCode.Home] = "home"; d[KeyCode.End] = "end";
            d[KeyCode.PageUp] = "page up"; d[KeyCode.PageDown] = "page down";
            d[KeyCode.UpArrow] = "up"; d[KeyCode.DownArrow] = "down"; d[KeyCode.LeftArrow] = "left"; d[KeyCode.RightArrow] = "right";
            d[KeyCode.LeftShift] = "left shift"; d[KeyCode.RightShift] = "right shift";
            d[KeyCode.LeftControl] = "left ctrl"; d[KeyCode.RightControl] = "right ctrl";
            d[KeyCode.LeftAlt] = "left alt"; d[KeyCode.RightAlt] = "right alt";
            d[KeyCode.Minus] = "-"; d[KeyCode.Equals] = "="; d[KeyCode.Comma] = ","; d[KeyCode.Period] = ".";
            d[KeyCode.Slash] = "/"; d[KeyCode.Backslash] = "\\"; d[KeyCode.Semicolon] = ";"; d[KeyCode.Quote] = "'";
            d[KeyCode.LeftBracket] = "["; d[KeyCode.RightBracket] = "]"; d[KeyCode.BackQuote] = "`";
            return d;
        }

        internal static string KeyName(KeyCode k) => keyNames.TryGetValue(k, out var n) ? n : "";

        static Exception InputSystemOnly(string what) => new InvalidOperationException(
            $"Input.{what}: you are reading input with the Input Manager, but Active Input Handling is set to the Input System only (Edit ▸ Project Settings ▸ Input Manager)");

        /// <summary>phase: 0 held, 1 pressed this frame, 2 released this frame.</summary>
        internal static bool Key(int phase, string name)
        {
            int r;
            fixed (char* p = name) r = api.inputKey(phase, p, name.Length);
            if (r < 0) throw InputSystemOnly("GetKey");
            return r == 1;
        }

        internal static bool Mouse(int phase, int button)
        {
            int r = api.inputMouse(phase, button);
            if (r < 0) throw InputSystemOnly("GetMouseButton");
            return r == 1;
        }

        internal static float Axis(int raw, string name)
        {
            float v;
            int r;
            fixed (char* p = name) r = api.inputAxis(raw, p, name.Length, &v);
            if (r == -1) throw InputSystemOnly("GetAxis");
            if (r == 0) throw new ArgumentException($"Input Axis {name} is not setup. To change the input settings use: Edit ▸ Project Settings ▸ Input Manager");
            return v;
        }

        internal static bool Button(int phase, string name)
        {
            int r;
            fixed (char* p = name) r = api.inputButton(phase, p, name.Length);
            if (r == -1) throw InputSystemOnly("GetButton");
            if (r == -2) throw new ArgumentException($"Input Button {name} is not setup. To change the input settings use: Edit ▸ Project Settings ▸ Input Manager");
            return r == 1;
        }

        internal static Vector3 InputVector(int which)
        {
            Vector3 v;
            api.inputVector(which, &v);
            return v;
        }

        internal static bool InputFlag(int which) => api.inputFlag(which) == 1;

        internal static void LookAt(ulong id, Vector3 point)
        {
            if (api.lookAt(id, &point) == 0) throw Destroyed();
        }
    }

    /// <summary>Entry points the engine calls. Method ids follow `Methods`.</summary>
    public static unsafe class Host
    {
        static readonly string[] Methods =
        {
            "Awake", "OnEnable", "Start", "Update", "FixedUpdate", "LateUpdate", "OnDisable", "OnDestroy",
            "OnCollisionEnter", "OnCollisionExit", "OnTriggerEnter", "OnTriggerExit",
            "OnCollisionStay", "OnTriggerStay",
        };

        /// <summary>The first method id that takes the other object (contact callbacks).</summary>
        const int FirstContact = 8;

        sealed class Entry
        {
            public MonoBehaviour script;
            /// <summary>Per method: called with the other object's id (0 for none).</summary>
            public Action<ulong>[] calls;
            public string typeName;
        }

        static readonly List<Entry> entries = new List<Entry> { null };
        static readonly Stack<int> free = new Stack<int>();
        static readonly Dictionary<string, Type> types = new Dictionary<string, Type>();
        /// <summary>Scripts per object, in slot order.</summary>
        static readonly Dictionary<ulong, List<MonoBehaviour>> byObject = new Dictionary<ulong, List<MonoBehaviour>>();
        /// <summary>Destroy(obj, t): object id → game time to destroy at.</summary>
        static readonly List<(ulong id, float at)> timed = new List<(ulong, float)>();

        [UnmanagedCallersOnly]
        public static int Init(void* table)
        {
            Native.api = *(NativeApi*)table;
            Coroutines.Clear();
            types.Clear();
            byObject.Clear();
            timed.Clear();
            entries.Clear();
            entries.Add(null);
            free.Clear();
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

        /// <summary>A new instance of class `name` in slot `slot` of object `entity`; its handle,
        /// or 0 (the reason is in the Console).</summary>
        [UnmanagedCallersOnly]
        public static int Create(char* name, int len, ulong entity, int slot)
        {
            string typeName = new string(name, 0, len);
            try
            {
                if (!types.TryGetValue(typeName, out var type))
                {
                    Native.Log(2, $"{typeName}.cs: no class named {typeName} that derives from MonoBehaviour");
                    return 0;
                }
                var script = (MonoBehaviour)RuntimeHelpers_GetUninitialized(type);
                script.entity = entity;
                script.slot = slot;
                // Field initialisers and the constructor run with the object already known.
                type.GetConstructor(BindingFlags.Instance | BindingFlags.Public | BindingFlags.NonPublic, null, Type.EmptyTypes, null)
                    ?.Invoke(script, null);
                var calls = new Action<ulong>[Methods.Length];
                for (int i = 0; i < Methods.Length; i++)
                {
                    calls[i] = Bind(type, script, Methods[i], i >= FirstContact);
                }
                var entry = new Entry { script = script, calls = calls, typeName = typeName };
                int handle;
                if (free.Count > 0) { handle = free.Pop(); entries[handle] = entry; }
                else { entries.Add(entry); handle = entries.Count - 1; }
                script.handle = handle;
                if (!byObject.TryGetValue(entity, out var list)) byObject[entity] = list = new List<MonoBehaviour>();
                int at = list.FindIndex(s => s.slot > slot);
                if (at < 0) list.Add(script); else list.Insert(at, script);
                return handle;
            }
            catch (Exception e)
            {
                Report(typeName, "constructor", e);
                return 0;
            }
        }

        static object RuntimeHelpers_GetUninitialized(Type t) =>
            System.Runtime.CompilerServices.RuntimeHelpers.GetUninitializedObject(t);

        /// <summary>Callback `name` of the script as a call taking the other object's id: no
        /// parameters, or (for contact callbacks) one `Collision`, `Collider` or `GameObject`.
        /// Null when the class doesn't define it.</summary>
        static Action<ulong> Bind(Type type, MonoBehaviour script, string name, bool contact)
        {
            const BindingFlags flags = BindingFlags.Instance | BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.DeclaredOnly;
            for (var t = type; t != null && t != typeof(MonoBehaviour); t = t.BaseType)
            {
                foreach (var m in t.GetMethods(flags))
                {
                    if (m.Name != name || m.IsGenericMethod) continue;
                    var ps = m.GetParameters();
                    // `IEnumerator Start()` and friends run as a coroutine.
                    if (m.ReturnType == typeof(System.Collections.IEnumerator) && ps.Length == 0)
                    {
                        var f = (Func<System.Collections.IEnumerator>)Delegate.CreateDelegate(
                            typeof(Func<System.Collections.IEnumerator>), script, m);
                        return _ => script.StartCoroutine(f());
                    }
                    if (m.ReturnType != typeof(void)) continue;
                    if (ps.Length == 0)
                    {
                        var a = (Action)Delegate.CreateDelegate(typeof(Action), script, m);
                        return _ => a();
                    }
                    if (!contact || ps.Length != 1) continue;
                    var p = ps[0].ParameterType;
                    if (p == typeof(Collision))
                    {
                        var a = (Action<Collision>)Delegate.CreateDelegate(typeof(Action<Collision>), script, m);
                        return other => a(new Collision(other));
                    }
                    if (p == typeof(Collider))
                    {
                        var a = (Action<Collider>)Delegate.CreateDelegate(typeof(Action<Collider>), script, m);
                        return other => a(new Collider(other));
                    }
                    if (p == typeof(GameObject))
                    {
                        var a = (Action<GameObject>)Delegate.CreateDelegate(typeof(Action<GameObject>), script, m);
                        return other => a(new GameObject(other));
                    }
                }
            }
            return null;
        }

        /// <summary>Whether the script defines method `method` (so the engine can skip it).</summary>
        [UnmanagedCallersOnly]
        public static int Has(int handle, int method)
        {
            var e = Lookup(handle);
            return e != null && (uint)method < (uint)e.calls.Length && e.calls[method] != null ? 1 : 0;
        }

        /// <summary>Call `method`; contact callbacks get `other` (an object id).</summary>
        [UnmanagedCallersOnly]
        public static void Call(int handle, int method, ulong other)
        {
            if (pendingRefs.Count > 0) ResolvePending();
            Invoke(handle, method, other);
        }

        /// <summary>Advance every running coroutine (once per frame, after Update).</summary>
        [UnmanagedCallersOnly]
        public static void Tick() => Coroutines.Tick();

        [UnmanagedCallersOnly]
        public static void Release(int handle)
        {
            var e = Lookup(handle);
            if (e == null) return;
            var s = e.script;
            // Its coroutines stop with it.
            s.handle = 0;
            pendingRefs.RemoveAll(p => p.owner == s);
            if (byObject.TryGetValue(s.entity, out var list))
            {
                list.Remove(s);
                if (list.Count == 0) byObject.Remove(s.entity);
            }
            entries[handle] = null;
            free.Push(handle);
        }

        /// <summary>The clock for this frame; also destroys what `Destroy(obj, t)` timed.</summary>
        [UnmanagedCallersOnly]
        public static void SetTime(float delta, float unscaledDelta, float time, float fixedDelta, float timeScale, int frame)
        {
            Time.deltaTime = delta;
            Time.unscaledDeltaTime = unscaledDelta;
            Time.time = time;
            Time.fixedDeltaTime = fixedDelta;
            Time.timeScale = timeScale;
            Time.frameCount = frame;
            for (int i = timed.Count - 1; i >= 0; i--)
            {
                if (timed[i].at <= time)
                {
                    Native.Destroy(timed[i].id);
                    timed.RemoveAt(i);
                }
            }
        }

        /// <summary>Set the Inspector value of field `name`: kind 0 bool (num != 0), 1 number,
        /// 2 Vector3 (`v`), 3 text. Returns 1, or 0 when the class has no such field.</summary>
        [UnmanagedCallersOnly]
        public static int SetField(int handle, char* name, int nameLen, int kind, double num, Vector3* v, char* text, int textLen)
        {
            var e = Lookup(handle);
            if (e == null) return 0;
            string fieldName = new string(name, 0, nameLen);
            try
            {
                var f = FindField(e.script.GetType(), fieldName);
                if (f == null) return 0;
                if (kind == 4)
                {
                    // An object field: the entity's bits in v.x and v.y.
                    ulong lo = (uint)BitConverter.SingleToInt32Bits(v->x);
                    ulong hi = (uint)BitConverter.SingleToInt32Bits(v->y);
                    ulong target = lo | (hi << 32);
                    if (target == 0 || !Native.Alive(target)) { f.SetValue(e.script, null); return 1; }
                    var found = ComponentOf(target, f.FieldType);
                    if (found != null) { f.SetValue(e.script, found); return 1; }
                    // A script on the target not made yet (objects are set up one by one):
                    // set before the next callback runs.
                    if (typeof(MonoBehaviour).IsAssignableFrom(f.FieldType))
                    {
                        pendingRefs.Add((e.script, f, target));
                        return 1;
                    }
                    return 0;
                }
                object value = kind switch
                {
                    0 => num != 0,
                    1 => num,
                    2 => *v,
                    _ => new string(text, 0, textLen),
                };
                var t = f.FieldType;
                object converted;
                if (t == typeof(float)) converted = Convert.ToSingle(value);
                else if (t == typeof(double)) converted = Convert.ToDouble(value);
                else if (t == typeof(int)) converted = (int)Math.Round(Convert.ToDouble(value));
                else if (t == typeof(long)) converted = (long)Math.Round(Convert.ToDouble(value));
                else if (t == typeof(bool)) converted = value is bool b ? b : Convert.ToDouble(value) != 0;
                else if (t == typeof(string)) converted = value is string s ? s : Convert.ToString(value, System.Globalization.CultureInfo.InvariantCulture);
                else if (t == typeof(Vector3) && value is Vector3) converted = value;
                else if (t == typeof(Vector2) && value is Vector3 v3) converted = new Vector2(v3.x, v3.y);
                else return 0;
                f.SetValue(e.script, converted);
                return 1;
            }
            catch (Exception ex)
            {
                Report(e.typeName, $"field {fieldName}", ex);
                return 0;
            }
        }

        /// <summary>Object fields waiting for a script on their target.</summary>
        static readonly List<(MonoBehaviour owner, FieldInfo field, ulong target)> pendingRefs = new List<(MonoBehaviour, FieldInfo, ulong)>();

        static void ResolvePending()
        {
            for (int i = pendingRefs.Count - 1; i >= 0; i--)
            {
                var (owner, field, target) = pendingRefs[i];
                if (!Native.Alive(target)) { pendingRefs.RemoveAt(i); continue; }
                var found = ComponentOf(target, field.FieldType);
                if (found == null) continue;
                field.SetValue(owner, found);
                pendingRefs.RemoveAt(i);
            }
        }

        /// <summary>What an object field of type <paramref name="t"/> gets from the object: itself,
        /// its Transform, an engine component or a script, else null.</summary>
        static object ComponentOf(ulong entity, Type t)
        {
            if (t == typeof(GameObject) || t == typeof(Object)) return new GameObject(entity);
            if (t == typeof(Transform) || t == typeof(Component)) return new Transform(entity);
            if (EngineComponent(t, entity) is Component c) return c;
            if (byObject.TryGetValue(entity, out var list))
                foreach (var s in list) if (t.IsInstanceOfType(s)) return s;
            return null;
        }

        static FieldInfo FindField(Type type, string name)
        {
            const BindingFlags flags = BindingFlags.Instance | BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.DeclaredOnly;
            for (var t = type; t != null && t != typeof(MonoBehaviour); t = t.BaseType)
            {
                var f = t.GetField(name, flags);
                if (f != null && !f.IsStatic && !f.IsInitOnly && (f.IsPublic || f.GetCustomAttribute<SerializeField>() != null))
                    return f;
            }
            return null;
        }

        static Entry Lookup(int handle) => handle > 0 && handle < entries.Count ? entries[handle] : null;

        static void Invoke(int handle, int method, ulong other)
        {
            var e = Lookup(handle);
            if (e == null || (uint)method >= (uint)e.calls.Length) return;
            var call = e.calls[method];
            if (call == null) return;
            try
            {
                call(other);
            }
            catch (Exception ex)
            {
                Report(e.typeName, Methods[method], ex);
            }
        }

        internal static void Report(string typeName, string method, Exception ex)
        {
            if (ex is TargetInvocationException tie && tie.InnerException != null) ex = tie.InnerException;
            Native.Log(2, $"{typeName} ({method}): {ex.GetType().Name}: {ex.Message}\n{ex.StackTrace}");
        }

        // ---- helpers for the API -------------------------------------------------------------

        internal static void Destroy(Object obj, float t)
        {
            if (obj is null || !obj.Alive) return;
            if (obj is MonoBehaviour mb)
            {
                // Removing one script during Play isn't supported yet: switch it off instead.
                mb.enabled = false;
                Native.Log(1, $"Destroy({mb.GetType().Name}): removing a script during Play is not supported yet; it was disabled instead");
                return;
            }
            if (t > 0f) timed.Add((obj.ObjectId, Time.time + t));
            else Native.Destroy(obj.ObjectId);
        }

        /// <summary>The engine component of type `t` on `entity` (Rigidbody, Collider,
        /// AudioSource, Animator, Camera, Light, Renderer), or null.</summary>
        static Component EngineComponent(Type t, ulong entity)
        {
            int kind = t == typeof(Rigidbody) ? 0 : t == typeof(Collider) ? 1
                : t == typeof(AudioSource) ? 2 : t == typeof(Animator) ? 3
                : t == typeof(Camera) ? 4 : t == typeof(Light) ? 5
                : t == typeof(Renderer) || t == typeof(MeshRenderer) ? 6 : -1;
            if (kind < 0 || Native.api.hasComponent(entity, kind) == 0) return null;
            return kind switch
            {
                0 => new Rigidbody(entity),
                1 => new Collider(entity),
                2 => new AudioSource(entity),
                3 => new Animator(entity),
                4 => new Camera(entity),
                5 => new Light(entity),
                _ => new MeshRenderer(entity),
            };
        }

        internal static T GetComponent<T>(ulong entity) where T : class
        {
            if (!Native.Alive(entity)) return null;
            if (typeof(T) == typeof(Transform)) return new Transform(entity) as T;
            if (EngineComponent(typeof(T), entity) is T engine) return engine;
            if (byObject.TryGetValue(entity, out var list))
                foreach (var s in list) if (s is T t) return t;
            return null;
        }

        internal static T[] GetComponents<T>(ulong entity) where T : class
        {
            var found = new List<T>();
            if (Native.Alive(entity))
            {
                if (typeof(T) == typeof(Transform)) found.Add(new Transform(entity) as T);
                if (EngineComponent(typeof(T), entity) is T engine) found.Add(engine);
                if (byObject.TryGetValue(entity, out var list))
                    foreach (var s in list) if (s is T t) found.Add(t);
            }
            return found.ToArray();
        }

        internal static T GetComponentInChildren<T>(ulong entity) where T : class
        {
            if (!Native.Alive(entity)) return null;
            var mine = GetComponent<T>(entity);
            if (mine != null) return mine;
            int n = (int)Native.Relative(entity, 2, 0);
            for (int i = 0; i < n; i++)
            {
                var found = GetComponentInChildren<T>(Native.Relative(entity, 1, i));
                if (found != null) return found;
            }
            return null;
        }

        internal static T GetComponentInParent<T>(ulong entity) where T : class
        {
            for (ulong e = entity; e != 0 && Native.Alive(e); e = Native.Relative(e, 0, 0))
            {
                var found = GetComponent<T>(e);
                if (found != null) return found;
            }
            return null;
        }

        internal static MonoBehaviour GetComponentByName(ulong entity, string type)
        {
            if (byObject.TryGetValue(entity, out var list))
                foreach (var s in list) if (s.GetType().Name == type) return s;
            return null;
        }

        internal static T FindOfType<T>() where T : MonoBehaviour
        {
            foreach (var list in byObject.Values)
                foreach (var s in list)
                    if (s is T t && t.Alive && t.gameObject.activeInHierarchy) return t;
            return null;
        }

        internal static T[] FindAllOfType<T>() where T : MonoBehaviour
        {
            var found = new List<T>();
            foreach (var list in byObject.Values)
                foreach (var s in list)
                    if (s is T t && t.Alive && t.gameObject.activeInHierarchy) found.Add(t);
            return found.ToArray();
        }
    }
}

namespace Misty.Internal
{
    /// <summary>The running coroutines of every script, advanced once per frame (Tick).</summary>
    internal static class Coroutines
    {
        static readonly List<Coroutine> running = new List<Coroutine>();
        /// <summary>Coroutines started by Invoke / InvokeRepeating, per script (CancelInvoke).</summary>
        static readonly HashSet<Coroutine> invokes = new HashSet<Coroutine>();
        static readonly System.Diagnostics.Stopwatch clock = System.Diagnostics.Stopwatch.StartNew();

        static float Realtime => (float)clock.Elapsed.TotalSeconds;

        internal static void Clear()
        {
            running.Clear();
            invokes.Clear();
        }

        internal static Coroutine Start(MonoBehaviour owner, System.Collections.IEnumerator routine)
        {
            var c = new Coroutine(owner, routine);
            // Runs right away up to its first yield, like the familiar engine.
            Step(c);
            if (!c.done) running.Add(c);
            return c;
        }

        internal static void Stop(Coroutine c)
        {
            if (c == null) return;
            c.done = true;
            running.Remove(c);
            invokes.Remove(c);
        }

        internal static void StopAll(MonoBehaviour owner)
        {
            foreach (var c in running) if (c.owner == (object)owner) c.done = true;
            running.RemoveAll(c => c.done);
            invokes.RemoveWhere(c => c.done);
        }

        internal static void StopInvokes(MonoBehaviour owner)
        {
            foreach (var c in invokes) if (c.owner == (object)owner) { c.done = true; running.Remove(c); }
            invokes.RemoveWhere(c => c.done);
        }

        internal static System.Collections.IEnumerator Delayed(MonoBehaviour owner, string method, float time, float repeat)
        {
            var m = owner.GetType().GetMethod(method,
                BindingFlags.Instance | BindingFlags.Public | BindingFlags.NonPublic, null, Type.EmptyTypes, null);
            if (m == null)
            {
                Native.Log(1, $"Trying to Invoke method: {owner.GetType().Name}.{method} couldn't be called.");
                yield break;
            }
            if (time > 0f) yield return new WaitForSeconds(time);
            do
            {
                m.Invoke(owner, null);
                if (repeat < 0f) yield break;
                yield return new WaitForSeconds(MathF.Max(repeat, 0.0001f));
            } while (true);
        }

        /// <summary>Remember coroutines started by Invoke so CancelInvoke finds them.</summary>
        internal static void MarkInvoke(Coroutine c) { if (!c.done) invokes.Add(c); }

        internal static void Tick()
        {
            // Started during this pass: they ran up to their first yield already.
            var now = running.ToArray();
            foreach (var c in now)
            {
                if (c.done) continue;
                var owner = c.owner;
                // Gone with its script or object, or the object was switched off: it stops.
                if (owner.handle == 0 || !owner.Alive || !owner.gameObject.activeInHierarchy)
                {
                    c.done = true;
                    continue;
                }
                Step(c);
            }
            running.RemoveAll(c => c.done);
            invokes.RemoveWhere(c => c.done);
        }

        /// <summary>Whether `c` may go on now (its last yield is over).</summary>
        static bool Ready(Coroutine c)
        {
            switch (c.waiting)
            {
                case null:
                    return Time.frameCount > c.resumeFrame;
                case WaitForSeconds _:
                    return Time.time >= c.resumeAt;
                case WaitForSecondsRealtime _:
                    return Realtime >= c.resumeAt;
                case WaitUntil u:
                    return u.predicate();
                case WaitWhile w:
                    return !w.predicate();
                case Coroutine other:
                    return other.done;
                default:
                    return Time.frameCount > c.resumeFrame;
            }
        }

        static void Step(Coroutine c)
        {
            try
            {
                if (c.stack.Count > 0 && c.yielded && !Ready(c)) return;
                while (c.stack.Count > 0)
                {
                    var top = c.stack.Peek();
                    if (!top.MoveNext())
                    {
                        c.stack.Pop();
                        continue;
                    }
                    var y = top.Current;
                    c.waiting = y;
                    c.resumeFrame = Time.frameCount;
                    c.yielded = true;
                    switch (y)
                    {
                        case WaitForSeconds s:
                            c.resumeAt = Time.time + s.seconds;
                            break;
                        case WaitForSecondsRealtime r:
                            c.resumeAt = Realtime + r.seconds;
                            break;
                        case System.Collections.IEnumerator nested:
                            // `yield return Other()` (or a CustomYieldInstruction): runs inside this one.
                            c.waiting = null;
                            c.stack.Push(nested);
                            continue;
                    }
                    return;
                }
                c.done = true;
            }
            catch (Exception ex)
            {
                c.done = true;
                Host.Report(c.owner.GetType().Name, "coroutine", ex);
            }
        }
    }
}
