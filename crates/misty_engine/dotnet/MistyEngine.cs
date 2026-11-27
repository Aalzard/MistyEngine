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
        internal static float scale = 1f;
        /// <summary>How fast game time passes (0 pauses: deltaTime is 0, physics and animation
        /// stop; unscaledDeltaTime keeps going). deltaTime follows a change from the next frame.</summary>
        public static unsafe float timeScale
        {
            get => scale;
            set
            {
                if (float.IsNaN(value)) return;
                scale = Math.Clamp(value, 0f, 100f);
                Internal.Native.api.timeScale(scale);
            }
        }
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
        /// <summary>Fingers on the touch screen this frame (lifted ones count for one frame).</summary>
        public static int touchCount => (int)InputBridge.Call(10);
        /// <summary>Touch `index` (0 .. touchCount - 1), positions from the bottom-left.</summary>
        public static Touch GetTouch(int index) => InputBridge.GetTouch(index);
        public static Touch[] touches => InputBridge.Touches();
        /// <summary>A touch screen has been used this session.</summary>
        public static bool touchSupported => InputBridge.Call(12) == 1;
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

        /// <summary>Keep the object (its root) and its scripts through single scene loads.</summary>
        public static void DontDestroyOnLoad(Object obj)
        {
            if (obj != null) Internal.Native.ComponentOp(obj.ObjectId, 2, "");
        }

        /// <summary>The first script of type T in the scene (active objects first), or null.</summary>
        public static T FindObjectOfType<T>() where T : MonoBehaviour => Internal.Host.FindOfType<T>();
        public static T[] FindObjectsOfType<T>() where T : MonoBehaviour => Internal.Host.FindAllOfType<T>();
    }

    /// <summary>Moves many objects in one engine call: positions and rotations of the whole
    /// group as arrays (object i owns element i). For thousands of objects driven by one
    /// script instead of a script each. A destroyed object keeps its place (reads leave its
    /// element alone, writes skip it) until <see cref="Prune"/>.</summary>
    public sealed unsafe class TransformGroup
    {
        ulong[] ids = new ulong[16];
        int n;

        public TransformGroup() { }
        public TransformGroup(GameObject[] objects)
        {
            foreach (var o in objects) if (o) Add(o);
        }

        public int count => n;

        /// <summary>Adds an object; returns its place.</summary>
        public int Add(GameObject o)
        {
            if (!o) throw new ArgumentException("TransformGroup.Add: the object is null or destroyed");
            if (n == ids.Length) Array.Resize(ref ids, n * 2);
            ids[n] = o.id;
            return n++;
        }
        public int Add(Transform t) => Add(t ? t.gameObject : null);

        /// <summary>The object at place i, or null when it was destroyed.</summary>
        public GameObject Get(int i)
        {
            var o = new GameObject(ids[Check(i)]);
            return o ? o : null;
        }

        /// <summary>Removes place i; the last object moves into it (do the same with your own arrays).</summary>
        public void RemoveAt(int i)
        {
            ids[Check(i)] = ids[--n];
        }

        /// <summary>Drops destroyed objects. Returns the freed places in the order they were
        /// filled, each by the object that was last at that moment.</summary>
        public int[] Prune()
        {
            var alive = new float[n];
            fixed (float* p = alive) Internal.Native.TransformGroup(5, ids, n, p);
            var freed = new List<int>();
            for (int i = n - 1; i >= 0; i--)
            {
                if (alive[i] != 0f) continue;
                // From the end: the object moved into a freed place is known to be alive.
                ids[i] = ids[--n];
                freed.Add(i);
            }
            return freed.ToArray();
        }

        public void Clear() { n = 0; }

        /// <summary>World positions into `positions` (as many as fit).</summary>
        public void GetPositions(Vector3[] positions)
        {
            fixed (Vector3* p = positions) Internal.Native.TransformGroup(0, ids, Fit(positions.Length), (float*)p);
        }
        public void SetPositions(Vector3[] positions)
        {
            fixed (Vector3* p = positions) Internal.Native.TransformGroup(1, ids, Fit(positions.Length), (float*)p);
        }
        /// <summary>World rotations into `rotations` (as many as fit).</summary>
        public void GetRotations(Quaternion[] rotations)
        {
            fixed (Quaternion* p = rotations) Internal.Native.TransformGroup(2, ids, Fit(rotations.Length), (float*)p);
        }
        public void SetRotations(Quaternion[] rotations)
        {
            fixed (Quaternion* p = rotations) Internal.Native.TransformGroup(3, ids, Fit(rotations.Length), (float*)p);
        }
        /// <summary>Turns every object to face its direction (like LookAt(position + direction));
        /// a zero direction leaves it as it is.</summary>
        public void LookAlong(Vector3[] directions)
        {
            fixed (Vector3* p = directions) Internal.Native.TransformGroup(4, ids, Fit(directions.Length), (float*)p);
        }

        int Fit(int length) => length < n ? length : n;
        int Check(int i)
        {
            if (i < 0 || i >= n) throw new ArgumentOutOfRangeException(nameof(i), "TransformGroup has " + n + " objects");
            return i;
        }
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
        /// <summary>Add an engine component (Rigidbody, BoxCollider, Light, AudioSource, …) with
        /// its defaults and return it (the existing one when the object has it), or a script:
        /// returned at once, its Awake and OnEnable run right after the calling method.</summary>
        public T AddComponent<T>() where T : Component => (T)AddComponent(typeof(T));
        public Component AddComponent(Type type) => Internal.Native.AddComponent(id, type);

        /// <summary>Call method `methodName` on every script of this object (C# now, Lua right
        /// after this callback). A value C# can't hand to Lua (an object) reaches C# only.</summary>
        public void SendMessage(string methodName, object value = null, SendMessageOptions options = SendMessageOptions.RequireReceiver) =>
            Internal.Host.SendMessage(id, methodName, value, 0, options);
        public void SendMessage(string methodName, SendMessageOptions options) =>
            Internal.Host.SendMessage(id, methodName, null, 0, options);
        /// <summary>SendMessage to this object and everything under it.</summary>
        public void BroadcastMessage(string methodName, object value = null, SendMessageOptions options = SendMessageOptions.RequireReceiver) =>
            Internal.Host.SendMessage(id, methodName, value, 1, options);
        public void BroadcastMessage(string methodName, SendMessageOptions options) =>
            Internal.Host.SendMessage(id, methodName, null, 1, options);
        /// <summary>SendMessage to this object and each of its parents.</summary>
        public void SendMessageUpwards(string methodName, object value = null, SendMessageOptions options = SendMessageOptions.RequireReceiver) =>
            Internal.Host.SendMessage(id, methodName, value, 2, options);
        public void SendMessageUpwards(string methodName, SendMessageOptions options) =>
            Internal.Host.SendMessage(id, methodName, null, 2, options);

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

        /// <summary>Call method `methodName` on every script of this object (C# now, Lua right
        /// after this callback). A value C# can't hand to Lua (an object) reaches C# only.</summary>
        public void SendMessage(string methodName, object value = null, SendMessageOptions options = SendMessageOptions.RequireReceiver) =>
            Internal.Host.SendMessage(EntityId, methodName, value, 0, options);
        public void SendMessage(string methodName, SendMessageOptions options) =>
            Internal.Host.SendMessage(EntityId, methodName, null, 0, options);
        /// <summary>SendMessage to this object and everything under it.</summary>
        public void BroadcastMessage(string methodName, object value = null, SendMessageOptions options = SendMessageOptions.RequireReceiver) =>
            Internal.Host.SendMessage(EntityId, methodName, value, 1, options);
        public void BroadcastMessage(string methodName, SendMessageOptions options) =>
            Internal.Host.SendMessage(EntityId, methodName, null, 1, options);
        /// <summary>SendMessage to this object and each of its parents.</summary>
        public void SendMessageUpwards(string methodName, object value = null, SendMessageOptions options = SendMessageOptions.RequireReceiver) =>
            Internal.Host.SendMessage(EntityId, methodName, value, 2, options);
        public void SendMessageUpwards(string methodName, SendMessageOptions options) =>
            Internal.Host.SendMessage(EntityId, methodName, null, 2, options);
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

    [System.Flags]
    public enum RigidbodyConstraints
    {
        None = 0,
        FreezePositionX = 2, FreezePositionY = 4, FreezePositionZ = 8,
        FreezeRotationX = 16, FreezeRotationY = 32, FreezeRotationZ = 64,
        FreezePosition = 14, FreezeRotation = 112, FreezeAll = 126,
    }

    public enum CollisionDetectionMode { Discrete, Continuous, ContinuousDynamic, ContinuousSpeculative }
    public enum RigidbodyInterpolation { None, Interpolate, Extrapolate }

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
        public float drag { get => Scalar(3); set => SetScalar(3, value); }
        public float angularDrag { get => Scalar(4); set => SetScalar(4, value); }
        public float linearDamping { get => drag; set => drag = value; }
        public float angularDamping { get => angularDrag; set => angularDrag = value; }
        public RigidbodyConstraints constraints { get => (RigidbodyConstraints)(int)Scalar(5); set => SetScalar(5, (int)value); }
        public bool freezeRotation
        {
            get => (constraints & RigidbodyConstraints.FreezeRotation) == RigidbodyConstraints.FreezeRotation;
            set => constraints = value ? constraints | RigidbodyConstraints.FreezeRotation : constraints & ~RigidbodyConstraints.FreezeRotation;
        }
        /// <summary>Every mode but Discrete turns on continuous collision detection.</summary>
        public CollisionDetectionMode collisionDetectionMode
        {
            get => Scalar(6) != 0 ? CollisionDetectionMode.Continuous : CollisionDetectionMode.Discrete;
            set => SetScalar(6, value == CollisionDetectionMode.Discrete ? 0 : 1);
        }
        /// <summary>Extrapolate works as Interpolate.</summary>
        public RigidbodyInterpolation interpolation
        {
            get => Scalar(7) != 0 ? RigidbodyInterpolation.Interpolate : RigidbodyInterpolation.None;
            set => SetScalar(7, value == RigidbodyInterpolation.None ? 0 : 1);
        }
        public Vector3 GetPointVelocity(Vector3 worldPoint)
        {
            Vector3* v = stackalloc Vector3[2];
            v[0] = worldPoint;
            if (Internal.Native.api.physicsExtra(10, id, 0, v, 0, 0, 0) != 1) throw Internal.Native.Missing("Rigidbody");
            return v[0];
        }
        public Vector3 worldCenterOfMass
        {
            get
            {
                Vector3* v = stackalloc Vector3[2];
                if (Internal.Native.api.physicsExtra(11, id, 0, v, 0, 0, 0) != 1) return position;
                return v[0];
            }
        }
        public void AddRelativeTorque(Vector3 torque, ForceMode mode = ForceMode.Force) => AddTorque(transform.rotation * torque, mode);
        /// <summary>The body's position in the physics world (the Transform may be interpolated).</summary>
        public Vector3 position => Vec(7);
        public Quaternion rotation => transform.rotation;

        public void AddForce(Vector3 force, ForceMode mode = ForceMode.Force) => SetVec(4, force, mode);
        public void AddForce(float x, float y, float z, ForceMode mode = ForceMode.Force) => AddForce(new Vector3(x, y, z), mode);
        public void AddTorque(Vector3 torque, ForceMode mode = ForceMode.Force) => SetVec(5, torque, mode);
        /// <summary>A force at a world point: also turns the body.</summary>
        public void AddForceAtPosition(Vector3 force, Vector3 position, ForceMode mode = ForceMode.Force)
        {
            Vector3* v = stackalloc Vector3[2];
            v[0] = force; v[1] = position;
            Internal.Native.api.physicsExtra(4, id, (ulong)mode, v, 0, 0, 0);
        }
        /// <summary>Pushes the body away from an explosion at `explosionPosition`, weaker towards
        /// `explosionRadius` (0: no limit); `upwardsModifier` lifts it.</summary>
        public void AddExplosionForce(float explosionForce, Vector3 explosionPosition, float explosionRadius, float upwardsModifier = 0f, ForceMode mode = ForceMode.Force)
        {
            Vector3* v = stackalloc Vector3[2];
            v[0] = explosionPosition;
            Internal.Native.api.physicsExtra(5, id, (ulong)mode, v, explosionForce, explosionRadius, upwardsModifier);
        }
        public void Sleep() => Internal.Native.api.physicsExtra(6, id, 0, null, 0, 0, 0);
        public void WakeUp() => Internal.Native.api.physicsExtra(7, id, 0, null, 0, 0, 0);
        public bool IsSleeping() => Internal.Native.api.physicsExtra(8, id, 0, null, 0, 0, 0) == 1;
        public void AddRelativeForce(Vector3 force, ForceMode mode = ForceMode.Force) => AddForce(transform.rotation * force, mode);
        /// <summary>Kinematic bodies sweep to `position` in the next step, pushing others.</summary>
        public void MovePosition(Vector3 position) => SetVec(6, position);
        public void MoveRotation(Quaternion rotation)
        {
            if (Internal.Native.api.rigidbodyMoveRotation(id, &rotation) == 0) throw Internal.Native.Missing("Rigidbody");
        }
    }

    /// <summary>The colliders of an object (Box, Sphere, Capsule or Mesh Collider).</summary>
    public class Collider : Component
    {
        internal readonly ulong id;
        internal Collider(ulong id) { this.id = id; }
        internal override ulong EntityId => id;
        /// <summary>The Rigidbody this collider moves with (its own or a parent's), or null.</summary>
        public Rigidbody attachedRigidbody => GetComponentInParent<Rigidbody>();
        /// <summary>The point on this object's colliders nearest `position` (itself when inside).</summary>
        public unsafe Vector3 ClosestPoint(Vector3 position)
        {
            Vector3* v = stackalloc Vector3[2];
            v[0] = position;
            return Internal.Native.api.physicsExtra(9, EntityId, 0, v, 0, 0, 0) == 1 ? v[0] : position;
        }
    }

    public sealed class BoxCollider : Collider { internal BoxCollider(ulong id) : base(id) { } }
    public sealed class SphereCollider : Collider { internal SphereCollider(ulong id) : base(id) { } }
    public sealed class CapsuleCollider : Collider { internal CapsuleCollider(ulong id) : base(id) { } }
    public sealed class MeshCollider : Collider { internal MeshCollider(ulong id) : base(id) { } }

    /// <summary>What an OnCollisionEnter / OnCollisionExit callback hit.</summary>
    public sealed partial class Collision
    {
        readonly ulong id;
        internal Collision(ulong id) { this.id = id; Capture(); }
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

        // ---- box and capsule queries (shape_query in csharp_native.rs) ----

        const int Sphere = 0, Box = 1, Capsule = 2;

        static int Query(int op, int kind, Vector3 a, Vector3 b, float radius, Quaternion rot, Vector3 dir, float max, int layerMask,
            Internal.HitData* hits, ulong* ids, int cap)
        {
            float* p = stackalloc float[15];
            p[0] = a.x; p[1] = a.y; p[2] = a.z; p[3] = b.x; p[4] = b.y; p[5] = b.z; p[6] = radius;
            p[7] = rot.x; p[8] = rot.y; p[9] = rot.z; p[10] = rot.w;
            p[11] = dir.x; p[12] = dir.y; p[13] = dir.z; p[14] = max;
            int r = Internal.Native.api.shapeQuery(op, kind, p, (uint)layerMask, hits, ids, cap);
            if (r < 0) throw new InvalidOperationException("Physics used outside a script callback");
            return r;
        }

        static bool Check(int kind, Vector3 a, Vector3 b, float radius, Quaternion rot, int layerMask) =>
            Query(0, kind, a, b, radius, rot, Vector3.zero, 0, layerMask, null, null, 0) == 1;

        static Collider[] Overlap(int kind, Vector3 a, Vector3 b, float radius, Quaternion rot, int layerMask)
        {
            var buf = new ulong[64];
            while (true)
            {
                int n;
                fixed (ulong* ids = buf) n = Query(1, kind, a, b, radius, rot, Vector3.zero, 0, layerMask, null, ids, buf.Length);
                if (n > buf.Length) { buf = new ulong[n]; continue; }
                var found = new Collider[n];
                for (int i = 0; i < n; i++) found[i] = new Collider(buf[i]);
                return found;
            }
        }

        static bool Sweep(int kind, Vector3 a, Vector3 b, float radius, Quaternion rot, Vector3 dir, out RaycastHit hit, float max, int layerMask)
        {
            Internal.HitData data;
            int r = Query(2, kind, a, b, radius, rot, dir, max, layerMask, &data, null, 1);
            hit = new RaycastHit { data = r == 1 ? data : default };
            return r == 1;
        }

        static RaycastHit[] SweepAll(int kind, Vector3 a, Vector3 b, float radius, Quaternion rot, Vector3 dir, float max, int layerMask)
        {
            var buf = new Internal.HitData[64];
            int n;
            fixed (Internal.HitData* h = buf) n = Query(3, kind, a, b, radius, rot, dir, max, layerMask, h, null, buf.Length);
            n = Math.Min(n, buf.Length);
            var hits = new RaycastHit[n];
            for (int i = 0; i < n; i++) hits[i] = new RaycastHit { data = buf[i] };
            return hits;
        }

        /// <summary>The colliders of `collider1` and `collider2` pass through each other.</summary>
        public static void IgnoreCollision(Collider collider1, Collider collider2, bool ignore = true) =>
            Internal.Native.api.physicsExtra(0, collider1.EntityId, collider2.EntityId, null, ignore ? 1 : 0, 0, 0);
        public static bool GetIgnoreCollision(Collider collider1, Collider collider2) =>
            Internal.Native.api.physicsExtra(1, collider1.EntityId, collider2.EntityId, null, 0, 0, 0) == 1;
        /// <summary>Tick or untick a box of the Layer Collision Matrix while playing.</summary>
        public static void IgnoreLayerCollision(int layer1, int layer2, bool ignore = true) =>
            Internal.Native.api.physicsExtra(2, (ulong)Math.Clamp(layer1, 0, 31), (ulong)Math.Clamp(layer2, 0, 31), null, ignore ? 1 : 0, 0, 0);
        public static bool GetIgnoreLayerCollision(int layer1, int layer2) =>
            Internal.Native.api.physicsExtra(3, (ulong)Math.Clamp(layer1, 0, 31), (ulong)Math.Clamp(layer2, 0, 31), null, 0, 0, 0) == 1;

        /// <summary>Whether any collider touches the sphere.</summary>
        public static bool CheckSphere(Vector3 position, float radius, int layerMask = DefaultRaycastLayers) =>
            Check(Sphere, position, Vector3.zero, radius, Quaternion.identity, layerMask);
        /// <summary>Whether any collider touches the box.</summary>
        public static bool CheckBox(Vector3 center, Vector3 halfExtents, Quaternion orientation = default, int layerMask = DefaultRaycastLayers) =>
            Check(Box, center, halfExtents, 0, orientation, layerMask);
        /// <summary>Whether any collider touches the capsule round start–end.</summary>
        public static bool CheckCapsule(Vector3 start, Vector3 end, float radius, int layerMask = DefaultRaycastLayers) =>
            Check(Capsule, start, end, radius, Quaternion.identity, layerMask);
        /// <summary>The colliders touching the box.</summary>
        public static Collider[] OverlapBox(Vector3 center, Vector3 halfExtents, Quaternion orientation = default, int layerMask = AllLayers) =>
            Overlap(Box, center, halfExtents, 0, orientation, layerMask);
        /// <summary>The colliders touching the capsule round point0–point1.</summary>
        public static Collider[] OverlapCapsule(Vector3 point0, Vector3 point1, float radius, int layerMask = AllLayers) =>
            Overlap(Capsule, point0, point1, radius, Quaternion.identity, layerMask);
        public static bool BoxCast(Vector3 center, Vector3 halfExtents, Vector3 direction, Quaternion orientation = default, float maxDistance = Mathf.Infinity, int layerMask = DefaultRaycastLayers) =>
            Sweep(Box, center, halfExtents, 0, orientation, direction, out _, maxDistance, layerMask);
        public static bool BoxCast(Vector3 center, Vector3 halfExtents, Vector3 direction, out RaycastHit hitInfo, Quaternion orientation = default, float maxDistance = Mathf.Infinity, int layerMask = DefaultRaycastLayers) =>
            Sweep(Box, center, halfExtents, 0, orientation, direction, out hitInfo, maxDistance, layerMask);
        public static bool CapsuleCast(Vector3 point1, Vector3 point2, float radius, Vector3 direction, float maxDistance = Mathf.Infinity, int layerMask = DefaultRaycastLayers) =>
            Sweep(Capsule, point1, point2, radius, Quaternion.identity, direction, out _, maxDistance, layerMask);
        public static bool CapsuleCast(Vector3 point1, Vector3 point2, float radius, Vector3 direction, out RaycastHit hitInfo, float maxDistance = Mathf.Infinity, int layerMask = DefaultRaycastLayers) =>
            Sweep(Capsule, point1, point2, radius, Quaternion.identity, direction, out hitInfo, maxDistance, layerMask);
        public static bool SphereCast(Ray ray, float radius, out RaycastHit hitInfo, float maxDistance = Mathf.Infinity, int layerMask = DefaultRaycastLayers) =>
            SphereCast(ray.origin, radius, ray.direction, out hitInfo, maxDistance, layerMask);
        /// <summary>Every collider the sweep hits, nearest first (at most 64).</summary>
        public static RaycastHit[] SphereCastAll(Vector3 origin, float radius, Vector3 direction, float maxDistance = Mathf.Infinity, int layerMask = DefaultRaycastLayers) =>
            SweepAll(Sphere, origin, Vector3.zero, radius, Quaternion.identity, direction, maxDistance, layerMask);
        public static RaycastHit[] BoxCastAll(Vector3 center, Vector3 halfExtents, Vector3 direction, Quaternion orientation = default, float maxDistance = Mathf.Infinity, int layerMask = DefaultRaycastLayers) =>
            SweepAll(Box, center, halfExtents, 0, orientation, direction, maxDistance, layerMask);
        public static RaycastHit[] CapsuleCastAll(Vector3 point1, Vector3 point2, float radius, Vector3 direction, float maxDistance = Mathf.Infinity, int layerMask = DefaultRaycastLayers) =>
            SweepAll(Capsule, point1, point2, radius, Quaternion.identity, direction, maxDistance, layerMask);

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

    /// <summary>The object's Character Controller: a capsule moved with Move / SimpleMove that
    /// slides along walls and climbs steps and slopes.</summary>
    public sealed unsafe class CharacterController : Component
    {
        internal readonly ulong id;
        internal CharacterController(ulong id) { this.id = id; }
        internal override ulong EntityId => id;

        Vector3 Vec(int op, Vector3 v = default)
        {
            if (Internal.Native.api.characterVec(id, op, &v) == 0) throw Internal.Native.Missing("CharacterController");
            return v;
        }

        float Get(int which)
        {
            float v;
            if (Internal.Native.api.characterScalar(id, which, 0, 0, &v) == 0) throw Internal.Native.Missing("CharacterController");
            return v;
        }

        void Set(int which, float v)
        {
            if (Internal.Native.api.characterScalar(id, which, 1, v, null) == 0) throw Internal.Native.Missing("CharacterController");
        }

        /// <summary>Move by `motion` (world space, no gravity added) with collisions. Returns
        /// whether the controller is grounded afterwards.</summary>
        public bool Move(Vector3 motion) => Vec(0, motion).x != 0;
        /// <summary>Walk at `speed` units per second (Y ignored) with gravity. Returns whether
        /// the controller is grounded.</summary>
        public bool SimpleMove(Vector3 speed) => Vec(1, speed).x != 0;
        /// <summary>Touching the ground after the last move.</summary>
        public bool isGrounded => Get(0) != 0;
        /// <summary>The last move's displacement per second.</summary>
        public Vector3 velocity => Vec(2);
        /// <summary>Normal of the ground under the controller after the last move (up while
        /// airborne).</summary>
        public Vector3 groundNormal => Vec(3);
        public Vector3 center { get => Vec(4); set => Vec(5, value); }
        public float height { get => Get(1); set => Set(1, value); }
        public float radius { get => Get(2); set => Set(2, value); }
        /// <summary>Steepest slope (degrees) the controller walks up.</summary>
        public float slopeLimit { get => Get(3); set => Set(3, value); }
        public float stepOffset { get => Get(4); set => Set(4, value); }
        public float skinWidth { get => Get(5); set => Set(5, value); }
        public float minMoveDistance { get => Get(6); set => Set(6, value); }
        /// <summary>How much walking speed follows the ground's incline (0 = not at all, 1 =
        /// like people hiking): slower uphill, a little faster down gentle slopes, careful on
        /// steep descents.</summary>
        public float slopeSpeed { get => Get(7); set => Set(7, value); }
    }

    /// <summary>The object's Animator: parameters and states of its controller.</summary>
    public sealed unsafe partial class Animator : Component
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
        /// <summary>The part of the screen the camera draws to, 0..1 from the bottom-left
        /// (split screen, picture in picture).</summary>
        public Rect rect { get => InputBridge.CameraRect(id, 15); set => InputBridge.SetCameraRect(id, value); }
        /// <summary>`rect` in pixels.</summary>
        public Rect pixelRect => InputBridge.CameraRect(id, 17);
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

    // ---- 2D physics ----------------------------------------------------------------------------

    public enum ForceMode2D { Force, Impulse }

    /// <summary>A 2D physics body (the object's Rigidbody 2D).</summary>
    public sealed unsafe class Rigidbody2D : Component
    {
        internal readonly ulong id;
        internal Rigidbody2D(ulong id) { this.id = id; }
        internal override ulong EntityId => id;

        Vector3 Op(int op, Vector3 v = default, int mode = 0)
        {
            if (Internal.Native.api.rigidbody2d(id, op, &v, mode) == 0) throw Internal.Native.Missing("Rigidbody2D");
            return v;
        }

        public Vector2 velocity { get => Op(0); set => Op(1, value); }
        public Vector2 position => Op(2);
        /// <summary>Degrees per second, counter-clockwise positive.</summary>
        public float angularVelocity { get => Op(5).x; set => Op(6, new Vector3(value, 0, 0)); }
        public float gravityScale { get => Op(7).x; set => Op(8, new Vector3(value, 0, 0)); }
        public float mass => Op(9).x;
        /// <summary>Degrees.</summary>
        public float rotation => Op(10).x;
        public void AddForce(Vector2 force, ForceMode2D mode = ForceMode2D.Force) => Op(3, force, (int)mode);
        public void AddTorque(float torque, ForceMode2D mode = ForceMode2D.Force) => Op(12, new Vector3(torque, 0, 0), (int)mode);
        public void MovePosition(Vector2 position) => Op(4, position);
        public void MoveRotation(float angle) => Op(11, new Vector3(angle, 0, 0));
    }

    /// <summary>The 2D colliders of an object (Box, Circle, Capsule, Polygon or Edge Collider 2D).</summary>
    public class Collider2D : Component
    {
        internal readonly ulong id;
        internal Collider2D(ulong id) { this.id = id; }
        internal override ulong EntityId => id;
        /// <summary>The Rigidbody 2D this collider moves with (its own or a parent's), or null.</summary>
        public Rigidbody2D attachedRigidbody => GetComponentInParent<Rigidbody2D>();
    }

    public sealed class BoxCollider2D : Collider2D { internal BoxCollider2D(ulong id) : base(id) { } }
    public sealed class CircleCollider2D : Collider2D { internal CircleCollider2D(ulong id) : base(id) { } }

    /// <summary>What an OnCollisionEnter2D / Stay2D / Exit2D callback hit.</summary>
    public sealed partial class Collision2D
    {
        readonly ulong id;
        internal Collision2D(ulong id) { this.id = id; Capture(); }
        public GameObject gameObject => new GameObject(id);
        public Transform transform => new Transform(id);
        public Collider2D collider => new Collider2D(id);
        public Rigidbody2D rigidbody => gameObject.GetComponentInParent<Rigidbody2D>();
    }

    /// <summary>What a Physics2D ray hit; false (no collider) when it hit nothing.</summary>
    public struct RaycastHit2D
    {
        internal Internal.HitData data;
        internal float rayLength;
        public Vector2 point => data.point;
        public Vector2 normal => data.normal;
        public float distance => data.distance;
        /// <summary>Distance as a fraction of the ray length (0 for an infinite ray).</summary>
        public float fraction => float.IsInfinity(rayLength) || rayLength <= 0 ? 0 : data.distance / rayLength;
        public Collider2D collider => data.entity == 0 ? null : new Collider2D(data.entity);
        public Rigidbody2D rigidbody => data.body == 0 ? null : new Rigidbody2D(data.body);
        public Transform transform => data.entity == 0 ? null : new Transform(data.entity);
        public static implicit operator bool(RaycastHit2D hit) => hit.data.entity != 0;
    }

    /// <summary>2D physics queries (the 2D world, not the 3D one).</summary>
    public static unsafe class Physics2D
    {
        public const int DefaultRaycastLayers = ~(1 << 2);
        public const int AllLayers = ~0;
        public const int IgnoreRaycastLayer = 1 << 2;

        static RaycastHit2D[] Rays(Vector2 origin, Vector2 direction, float distance, int layerMask)
        {
            var buf = new Internal.HitData[16];
            Vector3 o = origin, d = direction;
            while (true)
            {
                int n;
                fixed (Internal.HitData* b = buf) n = Internal.Native.api.physics2d(0, &o, &d, distance, (uint)layerMask, b, null, buf.Length);
                if (n > buf.Length) { buf = new Internal.HitData[n]; continue; }
                var hits = new RaycastHit2D[n];
                for (int i = 0; i < n; i++) hits[i] = new RaycastHit2D { data = buf[i], rayLength = distance };
                return hits;
            }
        }

        static Collider2D[] Overlap(int op, Vector2 a, Vector3 b, float num, int layerMask)
        {
            var buf = new ulong[32];
            Vector3 p = a;
            while (true)
            {
                int n;
                fixed (ulong* ids = buf) n = Internal.Native.api.physics2d(op, &p, &b, num, (uint)layerMask, null, ids, buf.Length);
                if (n > buf.Length) { buf = new ulong[n]; continue; }
                var found = new Collider2D[n];
                for (int i = 0; i < n; i++) found[i] = new Collider2D(buf[i]);
                return found;
            }
        }

        /// <summary>The nearest hit along the ray; test it like a bool.</summary>
        public static RaycastHit2D Raycast(Vector2 origin, Vector2 direction, float distance = Mathf.Infinity, int layerMask = DefaultRaycastLayers)
        {
            var all = Rays(origin, direction, distance, layerMask);
            return all.Length > 0 ? all[0] : default;
        }
        /// <summary>Every hit along the ray, nearest first.</summary>
        public static RaycastHit2D[] RaycastAll(Vector2 origin, Vector2 direction, float distance = Mathf.Infinity, int layerMask = DefaultRaycastLayers) =>
            Rays(origin, direction, distance, layerMask);
        public static RaycastHit2D Linecast(Vector2 start, Vector2 end, int layerMask = DefaultRaycastLayers) =>
            Raycast(start, end - start, (end - start).magnitude, layerMask);
        public static RaycastHit2D[] LinecastAll(Vector2 start, Vector2 end, int layerMask = DefaultRaycastLayers) =>
            Rays(start, end - start, (end - start).magnitude, layerMask);

        public static Collider2D OverlapCircle(Vector2 point, float radius, int layerMask = AllLayers)
        {
            var all = Overlap(1, point, default, radius, layerMask);
            return all.Length > 0 ? all[0] : null;
        }
        public static Collider2D[] OverlapCircleAll(Vector2 point, float radius, int layerMask = AllLayers) =>
            Overlap(1, point, default, radius, layerMask);
        /// <summary>`angle` in degrees.</summary>
        public static Collider2D OverlapBox(Vector2 point, Vector2 size, float angle, int layerMask = AllLayers)
        {
            var all = Overlap(2, point, new Vector3(size.x, size.y, 0), angle, layerMask);
            return all.Length > 0 ? all[0] : null;
        }
        public static Collider2D[] OverlapBoxAll(Vector2 point, Vector2 size, float angle, int layerMask = AllLayers) =>
            Overlap(2, point, new Vector3(size.x, size.y, 0), angle, layerMask);
        public static Collider2D OverlapPoint(Vector2 point, int layerMask = AllLayers)
        {
            var all = Overlap(3, point, default, 0, layerMask);
            return all.Length > 0 ? all[0] : null;
        }
        public static Collider2D[] OverlapPointAll(Vector2 point, int layerMask = AllLayers) =>
            Overlap(3, point, default, 0, layerMask);
    }

    // ---- navigation and particles -----------------------------------------------------------

    public enum NavMeshPathStatus { PathComplete, PathPartial, PathInvalid }

    /// <summary>The object's Nav Mesh Agent: walks the baked nav mesh to a destination.</summary>
    public sealed unsafe class NavMeshAgent : Component
    {
        internal readonly ulong id;
        internal NavMeshAgent(ulong id) { this.id = id; }
        internal override ulong EntityId => id;

        int Call(int op, ref Vector3 v)
        {
            Vector3 x = v;
            int r = Internal.Native.api.navAgent(id, op, &x);
            if (r == 0) throw Internal.Native.Missing("NavMeshAgent");
            v = x;
            return r;
        }

        Vector3 Get(int op) { var v = Vector3.zero; Call(op, ref v); return v; }
        void Set(int op, float value) { var v = new Vector3(value, 0, 0); Call(op, ref v); }
        void Set(int op, Vector3 value) => Call(op, ref value);

        /// <summary>Plan a path to `target`; false when the agent isn't on the nav mesh.</summary>
        public bool SetDestination(Vector3 target) => Call(0, ref target) == 2;
        public Vector3 destination { get => Get(1); set => SetDestination(value); }
        public float remainingDistance => Get(2).x;
        public bool hasPath => Get(3).x != 0;
        public bool pathPending => Get(4).x != 0;
        public bool isOnNavMesh => Get(5).x != 0;
        public bool isStopped { get => Get(6).x != 0; set => Set(7, value ? 1 : 0); }
        public Vector3 velocity => Get(8);
        public Vector3 desiredVelocity => Get(9);
        public Vector3 nextPosition => Get(20);
        public NavMeshPathStatus pathStatus => (NavMeshPathStatus)(int)Get(21).x;
        /// <summary>Jump to the nav mesh point nearest `position` (clears the path).</summary>
        public bool Warp(Vector3 position) => Call(10, ref position) == 2;
        public void ResetPath() => Set(11, 0);
        public float speed { get => Get(12).x; set => Set(13, value); }
        public float stoppingDistance { get => Get(14).x; set => Set(15, value); }
        public float angularSpeed { get => Get(16).x; set => Set(17, value); }
        public float acceleration { get => Get(18).x; set => Set(19, value); }
        public bool enabled { get => Get(22).x != 0; set => Set(23, value ? 1 : 0); }
    }

    /// <summary>The object's Particle System (its live emitter).</summary>
    public sealed unsafe class ParticleSystem : Component
    {
        internal readonly ulong id;
        internal ParticleSystem(ulong id) { this.id = id; }
        internal override ulong EntityId => id;

        float Op(int op, float v = 0)
        {
            if (Internal.Native.api.particles(id, op, &v) == 0) throw Internal.Native.Missing("ParticleSystem");
            return v;
        }

        public void Play() => Op(0);
        public void Stop() => Op(1);
        public void Pause() => Op(2);
        public void Clear() => Op(3);
        /// <summary>Emit `count` particles now.</summary>
        public void Emit(int count) => Op(4, count);
        public bool isPlaying => Op(5) != 0;
        public bool isPaused => Op(6) != 0;
        public bool isStopped => Op(7) != 0;
        public int particleCount => (int)Op(8);
        public float time => Op(9);
    }

    // ---- UI ----------------------------------------------------------------------------------

    /// <summary>An event scripts listen to with AddListener (a Button's onClick).</summary>
    public sealed class UnityEvent
    {
        readonly ulong entity;
        readonly int kind;
        internal UnityEvent(ulong entity, int kind) { this.entity = entity; this.kind = kind; }
        public void AddListener(Action call) => Internal.UiEvents.Add(entity, kind, call);
        public void RemoveListener(Action call) => Internal.UiEvents.Remove(entity, kind, call);
        public void RemoveAllListeners() => Internal.UiEvents.Clear(entity, kind);
        /// <summary>Run the C# listeners now.</summary>
        public void Invoke() => Internal.UiEvents.Run(entity, kind, null);
    }

    /// <summary>An event with a value (a Slider's onValueChanged, an Input Field's onEndEdit).</summary>
    public sealed class UnityEvent<T>
    {
        readonly ulong entity;
        readonly int kind;
        internal UnityEvent(ulong entity, int kind) { this.entity = entity; this.kind = kind; }
        public void AddListener(Action<T> call) => Internal.UiEvents.Add(entity, kind, call);
        public void RemoveListener(Action<T> call) => Internal.UiEvents.Remove(entity, kind, call);
        public void RemoveAllListeners() => Internal.UiEvents.Clear(entity, kind);
        public void Invoke(T value) => Internal.UiEvents.Run(entity, kind, value);
    }

    /// <summary>A UI component (read and set through the engine).</summary>
    public abstract unsafe class UIBehaviour : Behaviour
    {
        internal UIBehaviour(ulong id) : base(id) { }

        internal string GetText()
        {
            char* buf = stackalloc char[256];
            int n = Internal.Native.api.componentText(id, Kind, 0, buf, 256);
            if (n < 0) throw Internal.Native.Missing(TypeName);
            if (n <= 256) return new string(buf, 0, n);
            var big = new char[n];
            fixed (char* p = big)
            {
                n = Internal.Native.api.componentText(id, Kind, 0, p, big.Length);
                return new string(p, 0, Math.Min(Math.Max(n, 0), big.Length));
            }
        }

        internal void SetText(string value)
        {
            value ??= "";
            int r;
            fixed (char* p = value) r = Internal.Native.api.componentText(id, Kind, 1, p, value.Length);
            if (r < 0) throw Internal.Native.Missing(TypeName);
        }
    }

    /// <summary>A UI Text.</summary>
    public sealed class Text : UIBehaviour
    {
        internal Text(ulong id) : base(id) { }
        internal override int Kind => 10;
        internal override string TypeName => "Text";
        public string text { get => GetText(); set => SetText(value); }
        public bool enabled { get => GetBool(0); set => SetBool(0, value); }
        public float fontSize { get => GetFloat(1); set => SetFloat(1, value); }
        public Color color { get => Get(2); set => Set(2, value); }
    }

    /// <summary>A UI Image.</summary>
    public sealed class Image : UIBehaviour
    {
        internal Image(ulong id) : base(id) { }
        internal override int Kind => 11;
        internal override string TypeName => "Image";
        public bool enabled { get => GetBool(0); set => SetBool(0, value); }
        public Color color { get => Get(1); set => Set(1, value); }
        /// <summary>0..1, for Filled images (health bars, cooldowns).</summary>
        public float fillAmount { get => GetFloat(2); set => SetFloat(2, value); }
        public bool raycastTarget { get => GetBool(3); set => SetBool(3, value); }
    }

    /// <summary>A UI Button.</summary>
    public sealed class Button : UIBehaviour
    {
        internal Button(ulong id) : base(id) { }
        internal override int Kind => 12;
        internal override string TypeName => "Button";
        public bool interactable { get => GetBool(0); set => SetBool(0, value); }
        public UnityEvent onClick => new UnityEvent(id, 0);
    }

    /// <summary>A UI Toggle (a check box).</summary>
    public sealed class Toggle : UIBehaviour
    {
        internal Toggle(ulong id) : base(id) { }
        internal override int Kind => 13;
        internal override string TypeName => "Toggle";
        public bool interactable { get => GetBool(0); set => SetBool(0, value); }
        public bool isOn { get => GetBool(1); set => SetBool(1, value); }
        public UnityEvent<bool> onValueChanged => new UnityEvent<bool>(id, 1);
    }

    /// <summary>A UI Slider.</summary>
    public sealed class Slider : UIBehaviour
    {
        internal Slider(ulong id) : base(id) { }
        internal override int Kind => 14;
        internal override string TypeName => "Slider";
        public bool interactable { get => GetBool(0); set => SetBool(0, value); }
        public float value { get => GetFloat(1); set => SetFloat(1, value); }
        public float minValue { get => GetFloat(2); set => SetFloat(2, value); }
        public float maxValue { get => GetFloat(3); set => SetFloat(3, value); }
        public bool wholeNumbers { get => GetBool(4); set => SetBool(4, value); }
        /// <summary>The value as 0..1 between minValue and maxValue.</summary>
        public float normalizedValue { get => GetFloat(5); set => SetFloat(5, value); }
        public UnityEvent<float> onValueChanged => new UnityEvent<float>(id, 1);
    }

    /// <summary>A UI Input Field.</summary>
    public sealed class InputField : UIBehaviour
    {
        internal InputField(ulong id) : base(id) { }
        internal override int Kind => 15;
        internal override string TypeName => "InputField";
        public bool interactable { get => GetBool(0); set => SetBool(0, value); }
        public bool readOnly { get => GetBool(1); set => SetBool(1, value); }
        /// <summary>The text (the field's character limit and content type apply).</summary>
        public string text { get => GetText(); set => SetText(value); }
        public UnityEvent<string> onValueChanged => new UnityEvent<string>(id, 1);
        public UnityEvent<string> onEndEdit => new UnityEvent<string>(id, 2);
    }

    // ---- PlayerPrefs ---------------------------------------------------------------------------

    /// <summary>Small values kept between runs of the game (settings, high scores).</summary>
    public static unsafe class PlayerPrefs
    {
        static int Op(int op, string key, double* num, char* buf, int len)
        {
            key ??= "";
            fixed (char* k = key) return Internal.Native.api.prefs(op, k, key.Length, num, buf, len);
        }

        static double Num(int op, string key, double value)
        {
            Op(op, key, &value, null, 0);
            return value;
        }

        public static int GetInt(string key, int defaultValue = 0) => (int)Num(0, key, defaultValue);
        public static void SetInt(string key, int value) => Num(1, key, value);
        public static float GetFloat(string key, float defaultValue = 0f) => (float)Num(2, key, defaultValue);
        public static void SetFloat(string key, float value) => Num(3, key, value);

        public static string GetString(string key, string defaultValue = "")
        {
            defaultValue ??= "";
            var buf = new char[Math.Max(256, defaultValue.Length)];
            defaultValue.CopyTo(0, buf, 0, defaultValue.Length);
            int n;
            fixed (char* p = buf) n = Op(4, key, null, p, defaultValue.Length);
            if (n <= buf.Length) return new string(buf, 0, n);
            // Longer than the buffer: the key exists, so ask again with room for it.
            buf = new char[n];
            fixed (char* p = buf) n = Op(4, key, null, p, buf.Length);
            return new string(buf, 0, Math.Min(n, buf.Length));
        }

        public static void SetString(string key, string value)
        {
            value ??= "";
            fixed (char* p = value) Op(5, key, null, p, value.Length);
        }

        public static bool HasKey(string key) => Op(6, key, null, null, 0) == 1;
        public static void DeleteKey(string key) => Op(7, key, null, null, 0);
        public static void DeleteAll() => Op(8, "", null, null, 0);
        /// <summary>Write the values to disk now (they are also saved when the game quits).</summary>
        public static void Save() => Op(9, "", null, null, 0);
    }

    // ---- scenes and the application ----------------------------------------------------------

    public enum LoadSceneMode { Single, Additive }

    /// <summary>Whether SendMessage logs an error when no script has the method.</summary>
    public enum SendMessageOptions { RequireReceiver, DontRequireReceiver }

    /// <summary>Scene loading (carried out after the frame), like Unity's SceneManager.</summary>
    public static unsafe class SceneManager
    {
        static long Request(int op, int index, string name, int track = 0)
        {
            name ??= "";
            fixed (char* p = name) return Internal.Native.api.request(op, index, p, name.Length, track);
        }

        public static void LoadScene(string sceneName, LoadSceneMode mode = LoadSceneMode.Single) =>
            Request(mode == LoadSceneMode.Additive ? 1 : 0, -1, sceneName);
        public static void LoadScene(int sceneBuildIndex, LoadSceneMode mode = LoadSceneMode.Single) =>
            Request(mode == LoadSceneMode.Additive ? 1 : 0, sceneBuildIndex, null);
        public static AsyncOperation LoadSceneAsync(string sceneName, LoadSceneMode mode = LoadSceneMode.Single) =>
            new AsyncOperation(Request(mode == LoadSceneMode.Additive ? 1 : 0, -1, sceneName, 1));
        public static AsyncOperation LoadSceneAsync(int sceneBuildIndex, LoadSceneMode mode = LoadSceneMode.Single) =>
            new AsyncOperation(Request(mode == LoadSceneMode.Additive ? 1 : 0, sceneBuildIndex, null, 1));
        public static AsyncOperation UnloadSceneAsync(string sceneName) => new AsyncOperation(Request(2, -1, sceneName, 1));
        public static AsyncOperation UnloadSceneAsync(int sceneBuildIndex) => new AsyncOperation(Request(2, sceneBuildIndex, null, 1));
    }

    /// <summary>A scene load or unload in progress (`LoadSceneAsync`); `yield return` it in a
    /// coroutine to wait for it. Loads happen between frames: `progress` is 0.9 until done
    /// (and while `allowSceneActivation` is false), then 1.</summary>
    public sealed unsafe class AsyncOperation : CustomYieldInstruction
    {
        static readonly List<AsyncOperation> pending = new List<AsyncOperation>();
        readonly long id;
        Action<AsyncOperation> completedHandlers;
        bool fired;

        internal AsyncOperation(long id)
        {
            this.id = id;
            if (id > 0) pending.Add(this);
        }

        int State => id > 0 ? Internal.Native.api.sceneOp(id, -1) : 2;
        public bool isDone => State == 2;
        public float progress => isDone ? 1f : 0.9f;
        public int priority { get; set; }
        public bool allowSceneActivation
        {
            get => State != 0;
            set { if (id > 0) Internal.Native.api.sceneOp(id, value ? 1 : 0); }
        }
        public override bool keepWaiting => !isDone;

        /// <summary>Runs once the operation is done (at once when it already is).</summary>
        public event Action<AsyncOperation> completed
        {
            add
            {
                if (fired || isDone) { fired = true; value?.Invoke(this); }
                else completedHandlers += value;
            }
            remove => completedHandlers -= value;
        }

        /// <summary>Start of a frame: fire `completed` for the operations that finished.</summary>
        internal static void Tick()
        {
            for (int i = 0; i < pending.Count; i++)
            {
                var op = pending[i];
                if (!op.isDone) continue;
                pending.RemoveAt(i--);
                op.fired = true;
                var handlers = op.completedHandlers;
                op.completedHandlers = null;
                try { handlers?.Invoke(op); }
                catch (Exception e) { Internal.Native.Log(2, "AsyncOperation.completed: " + e); }
            }
        }

        internal static void Reset() => pending.Clear();
    }

    public static unsafe class Application
    {
        /// <summary>Quit the game (stops Play mode in the editor).</summary>
        public static void Quit() => Internal.Native.api.request(3, -1, null, 0, 0);
        public static bool isPlaying => true;
        /// <summary>True in the editor's Play mode.</summary>
        public static bool isEditor => Internal.Native.api.app(3, 0, null, 0, null, 0) == 1;
        /// <summary>Whether the Build Profile defines this scripting define.</summary>
        public static bool HasDefine(string name)
        {
            name ??= "";
            fixed (char* p = name) return Internal.Native.api.app(2, 0, p, name.Length, null, 0) == 1;
        }
        /// <summary>A folder the game can always write to: the project folder in the editor,
        /// the per-user folder that holds PlayerPrefs and saves in a built game.</summary>
        public static string persistentDataPath
        {
            get
            {
                char* buf = stackalloc char[512];
                int n = Internal.Native.api.app(1, 0, null, 0, buf, 512);
                if (n < 0) throw new InvalidOperationException("Application.persistentDataPath used outside a script callback");
                if (n <= 512) return new string(buf, 0, n);
                var big = new char[n];
                fixed (char* p = big)
                {
                    n = Internal.Native.api.app(1, 0, null, 0, p, big.Length);
                    return new string(p, 0, Math.Min(Math.Max(n, 0), big.Length));
                }
            }
        }
    }

    /// <summary>Saves the game's image as a PNG.</summary>
    public static unsafe class ScreenCapture
    {
        /// <summary>Save the frame being drawn once it is finished. A relative name goes under
        /// Application.persistentDataPath; superSize above 1 scales the image up (at most 8).</summary>
        public static void CaptureScreenshot(string filename, int superSize = 1)
        {
            if (string.IsNullOrEmpty(filename)) throw new ArgumentException("CaptureScreenshot needs a file name");
            fixed (char* p = filename)
            {
                if (Internal.Native.api.app(0, superSize, p, filename.Length, null, 0) < 0)
                    throw new InvalidOperationException("ScreenCapture used outside a script callback");
            }
        }
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

    // ---- Event System, touches, Input System (csharp_input.rs) ----------------------------------

    /// <summary>A rectangle: x and y from the bottom-left, width and height (a Camera's rect is
    /// 0..1 of the screen).</summary>
    public struct Rect : IEquatable<Rect>
    {
        public float x, y, width, height;

        public Rect(float x, float y, float width, float height)
        {
            this.x = x; this.y = y; this.width = width; this.height = height;
        }

        public Rect(Vector2 position, Vector2 size) : this(position.x, position.y, size.x, size.y) { }

        public static Rect zero => new Rect(0, 0, 0, 0);
        public static Rect MinMaxRect(float xmin, float ymin, float xmax, float ymax) =>
            new Rect(xmin, ymin, xmax - xmin, ymax - ymin);

        public Vector2 position { get => new Vector2(x, y); set { x = value.x; y = value.y; } }
        public Vector2 size { get => new Vector2(width, height); set { width = value.x; height = value.y; } }
        public Vector2 center
        {
            get => new Vector2(x + width / 2f, y + height / 2f);
            set { x = value.x - width / 2f; y = value.y - height / 2f; }
        }
        public Vector2 min => new Vector2(xMin, yMin);
        public Vector2 max => new Vector2(xMax, yMax);
        public float xMin { get => x; set { float m = xMax; x = value; width = m - x; } }
        public float yMin { get => y; set { float m = yMax; y = value; height = m - y; } }
        public float xMax { get => x + width; set => width = value - x; }
        public float yMax { get => y + height; set => height = value - y; }

        public bool Contains(Vector2 point) =>
            point.x >= xMin && point.x < xMax && point.y >= yMin && point.y < yMax;
        public bool Contains(Vector3 point) => Contains(new Vector2(point.x, point.y));
        public bool Overlaps(Rect other) =>
            other.xMax > xMin && other.xMin < xMax && other.yMax > yMin && other.yMin < yMax;

        public static bool operator ==(Rect a, Rect b) =>
            a.x == b.x && a.y == b.y && a.width == b.width && a.height == b.height;
        public static bool operator !=(Rect a, Rect b) => !(a == b);
        public bool Equals(Rect other) => this == other;
        public override bool Equals(object obj) => obj is Rect r && this == r;
        public override int GetHashCode() => HashCode.Combine(x, y, width, height);
        public override string ToString() => $"(x:{x:F2}, y:{y:F2}, width:{width:F2}, height:{height:F2})";
    }

    /// <summary>Data of a UI event (`OnSelect(BaseEventData)`, `OnDeselect`, `OnCancel`).</summary>
    public class BaseEventData
    {
        internal BaseEventData() { }
        /// <summary>The object the Event System has selected.</summary>
        public GameObject selectedObject
        {
            get => EventSystem.current.currentSelectedGameObject;
            set => EventSystem.current.SetSelectedGameObject(value);
        }
        public bool used { get; private set; }
        public void Use() => used = true;
        public void Reset() => used = false;
    }

    /// <summary>Scripts hear `OnSelect(BaseEventData)` (or `OnSelect()`) when the UI selection
    /// moves to their object.</summary>
    public interface ISelectHandler { void OnSelect(BaseEventData eventData); }
    /// <summary>`OnDeselect(BaseEventData)` when the UI selection leaves the object.</summary>
    public interface IDeselectHandler { void OnDeselect(BaseEventData eventData); }
    /// <summary>`OnCancel(BaseEventData)` on the selected object when Cancel is pressed.</summary>
    public interface ICancelHandler { void OnCancel(BaseEventData eventData); }

    /// <summary>The UI selection that keyboard and gamepad navigation move, and the scene's
    /// Event System settings. A selection set by a script takes effect before the next frame's
    /// Update, but `currentSelectedGameObject` returns it at once.</summary>
    public sealed class EventSystem
    {
        static readonly EventSystem instance = new EventSystem();
        EventSystem() { }

        public static EventSystem current => instance;

        public GameObject currentSelectedGameObject => InputBridge.Wrap(InputBridge.Call(0));
        /// <summary>Select `selected` (null clears the selection); its scripts hear OnSelect and
        /// the old one's OnDeselect.</summary>
        public void SetSelectedGameObject(GameObject selected) => InputBridge.Call(1, InputBridge.Id(selected));
        public void SetSelectedGameObject(GameObject selected, BaseEventData pointer) => SetSelectedGameObject(selected);
        /// <summary>The object selected when the scene starts.</summary>
        public GameObject firstSelectedGameObject
        {
            get => InputBridge.Wrap(InputBridge.Call(2));
            set => InputBridge.Call(3, InputBridge.Id(value));
        }
        /// <summary>Whether keyboard and gamepad navigation (and Submit / Cancel) reach the UI.</summary>
        public bool sendNavigationEvents
        {
            get => InputBridge.Call(4) == 1;
            set => InputBridge.Call(5, 0, value ? 1 : 0);
        }
        /// <summary>The object holding the scene's Event System, or null.</summary>
        public GameObject gameObject => InputBridge.Wrap(InputBridge.Call(6));
        public bool alreadySelecting => false;
        public bool isFocused => true;
        /// <summary>The mouse is over a UI element that blocks raycasts (a click there is the
        /// UI's).</summary>
        public bool IsPointerOverGameObject() => InputBridge.Call(7, 0, -1) == 1;
        /// <summary>Touch `pointerId` (its fingerId) is over a UI element; -1 is the mouse.</summary>
        public bool IsPointerOverGameObject(int pointerId) => InputBridge.Call(7, 0, pointerId) == 1;
    }

    /// <summary>`button.Select()` and the other selectables: make it the UI selection.</summary>
    public static class SelectableExtensions
    {
        public static void Select(this Button b) => EventSystem.current.SetSelectedGameObject(b.gameObject);
        public static void Select(this Toggle t) => EventSystem.current.SetSelectedGameObject(t.gameObject);
        public static void Select(this Slider s) => EventSystem.current.SetSelectedGameObject(s.gameObject);
        public static void Select(this InputField f) => EventSystem.current.SetSelectedGameObject(f.gameObject);
    }

    /// <summary>Where a finger is in its gesture.</summary>
    public enum TouchPhase { Began, Moved, Stationary, Ended, Canceled }

    /// <summary>One finger on the screen this frame (`Input.GetTouch`). Positions are pixels
    /// from the bottom-left, like `Input.mousePosition`.</summary>
    public struct Touch
    {
        public int fingerId { get; set; }
        public Vector2 position { get; set; }
        public Vector2 rawPosition { get; set; }
        /// <summary>Movement since the last frame, in pixels (y up).</summary>
        public Vector2 deltaPosition { get; set; }
        public TouchPhase phase { get; set; }
        /// <summary>Taps in a row at this spot: 2 on the second tap of a double tap; 0 once
        /// lifted when the touch was held too long or moved too far to be a tap.</summary>
        public int tapCount { get; set; }
        public override string ToString() => $"Touch {fingerId} {phase} {position}";
    }

    // Input System controls: read-only snapshots of a control as of the moment they were read.

    /// <summary>A control of a device (a key, a stick, a touch).</summary>
    public class InputControl
    {
        internal InputControl(string path) { this.path = path ?? ""; }
        /// <summary>`/Keyboard/space`, `/Gamepad/leftStick`.</summary>
        public string path { get; }
        public string name => path.Substring(path.LastIndexOf('/') + 1);
        public string displayName => name;
        public override string ToString() => path;
    }

    /// <summary>A one-dimensional control (a trigger, a stick's x).</summary>
    public class AxisControl : InputControl
    {
        internal readonly float value;
        internal AxisControl(string path, float value) : base(path) { this.value = value; }
        public float ReadValue() => value;
    }

    /// <summary>A button (a key, a gamepad button, a trigger past its press point).</summary>
    public sealed class ButtonControl : AxisControl
    {
        internal ButtonControl(string path, float value, bool held, bool pressed, bool released) : base(path, value)
        {
            isPressed = held; wasPressedThisFrame = pressed; wasReleasedThisFrame = released;
        }
        internal ButtonControl(string path, long bits) : this(path, (bits & 1) != 0 ? 1f : 0f, (bits & 1) != 0, (bits & 2) != 0, (bits & 4) != 0) { }
        public bool isPressed { get; }
        public bool wasPressedThisFrame { get; }
        public bool wasReleasedThisFrame { get; }
    }

    /// <summary>A whole-number control (a touch's id).</summary>
    public sealed class IntegerControl : InputControl
    {
        readonly int value;
        internal IntegerControl(string path, int value) : base(path) { this.value = value; }
        public int ReadValue() => value;
    }

    /// <summary>A touch's phase.</summary>
    public sealed class TouchPhaseControl : InputControl
    {
        readonly TouchPhase value;
        internal TouchPhaseControl(string path, TouchPhase value) : base(path) { this.value = value; }
        public TouchPhase ReadValue() => value;
    }

    /// <summary>A two-dimensional control (a stick, the d-pad, the mouse position).</summary>
    public class Vector2Control : InputControl
    {
        readonly Vector2 value;
        internal Vector2Control(string path, Vector2 value) : base(path) { this.value = value; }
        public Vector2 ReadValue() => value;
        public AxisControl x => new AxisControl(path + "/x", value.x);
        public AxisControl y => new AxisControl(path + "/y", value.y);
        /// <summary>The direction buttons (held past half way).</summary>
        public ButtonControl up => Dir(path + "/up", value.y);
        public ButtonControl down => Dir(path + "/down", -value.y);
        public ButtonControl left => Dir(path + "/left", -value.x);
        public ButtonControl right => Dir(path + "/right", value.x);
        static ButtonControl Dir(string p, float v) => new ButtonControl(p, Math.Max(v, 0f), v >= 0.5f, false, false);
    }

    /// <summary>An input device (`Gamepad`, `Keyboard`, `Mouse`, `Touchscreen`).</summary>
    public abstract class InputDevice
    {
        internal abstract string DeviceKey { get; }
        public abstract string name { get; }
        public string displayName => name;
        public override bool Equals(object obj) => obj is InputDevice d && d.DeviceKey == DeviceKey;
        public override int GetHashCode() => DeviceKey.GetHashCode();
        public static bool operator ==(InputDevice a, InputDevice b) => a is null ? b is null : a.Equals(b);
        public static bool operator !=(InputDevice a, InputDevice b) => !(a == b);
        public override string ToString() => name;
    }

    /// <summary>Gamepad buttons, with Xbox and PlayStation names.</summary>
    public enum GamepadButton
    {
        DpadUp = 0, DpadDown = 1, DpadLeft = 2, DpadRight = 3,
        North = 4, East = 5, South = 6, West = 7,
        LeftStick = 8, RightStick = 9, LeftShoulder = 10, RightShoulder = 11,
        Start = 12, Select = 13, LeftTrigger = 32, RightTrigger = 33,
        Y = North, B = East, A = South, X = West,
        Triangle = North, Circle = East, Cross = South, Square = West,
    }

    /// <summary>A gamepad (`Gamepad.current`, `Gamepad.all`, `playerInput.GetDevice&lt;Gamepad&gt;()`).
    /// Sticks read +1 up.</summary>
    public sealed unsafe class Gamepad : InputDevice
    {
        internal readonly int slot;
        internal Gamepad(int slot) { this.slot = slot; }
        internal override string DeviceKey => "Gamepad" + slot;

        /// <summary>The gamepad used most recently, or null.</summary>
        public static Gamepad current
        {
            get { long s = InputBridge.Call(90); return s < 0 ? null : new Gamepad((int)s); }
        }
        /// <summary>Every connected gamepad.</summary>
        public static Gamepad[] all
        {
            get
            {
                int n = (int)InputBridge.Call(91);
                var list = new List<Gamepad>();
                for (int i = 0; i < n; i++)
                {
                    long s = InputBridge.Call(92, 0, i);
                    if (s >= 0) list.Add(new Gamepad((int)s));
                }
                return list.ToArray();
            }
        }

        public override string name => InputBridge.Text(95, (ulong)slot, 0);
        /// <summary>The joystick slot (0-based).</summary>
        public int index => slot;

        Vector2 Axes(int at)
        {
            double* v = stackalloc double[16];
            InputBridge.Check(Internal.Native.api.inputSystem(93, (ulong)slot, 0, v, null, 0));
            return new Vector2((float)v[at], (float)v[at + 1]);
        }

        ButtonControl Button(string control, int index) =>
            new ButtonControl("/Gamepad/" + control, InputBridge.Check(InputBridge.Call(94, (ulong)slot, index)));

        ButtonControl Trigger(string control, float value) =>
            new ButtonControl("/Gamepad/" + control, value, value >= 0.5f, false, false);

        public Vector2Control leftStick => new Vector2Control("/Gamepad/leftStick", Axes(0));
        public Vector2Control rightStick => new Vector2Control("/Gamepad/rightStick", Axes(2));
        public Vector2Control dpad => new Vector2Control("/Gamepad/dpad", Axes(4));
        /// <summary>0..1; isPressed past half way.</summary>
        public ButtonControl leftTrigger => Trigger("leftTrigger", Axes(6).x);
        public ButtonControl rightTrigger => Trigger("rightTrigger", Axes(6).y);
        public ButtonControl buttonSouth => Button("buttonSouth", 0);
        public ButtonControl buttonEast => Button("buttonEast", 1);
        public ButtonControl buttonWest => Button("buttonWest", 2);
        public ButtonControl buttonNorth => Button("buttonNorth", 3);
        public ButtonControl leftShoulder => Button("leftShoulder", 4);
        public ButtonControl rightShoulder => Button("rightShoulder", 5);
        public ButtonControl selectButton => Button("select", 6);
        public ButtonControl startButton => Button("start", 7);
        public ButtonControl leftStickButton => Button("leftStickPress", 8);
        public ButtonControl rightStickButton => Button("rightStickPress", 9);
        public ButtonControl aButton => buttonSouth;
        public ButtonControl bButton => buttonEast;
        public ButtonControl xButton => buttonWest;
        public ButtonControl yButton => buttonNorth;
        public ButtonControl crossButton => buttonSouth;
        public ButtonControl circleButton => buttonEast;
        public ButtonControl squareButton => buttonWest;
        public ButtonControl triangleButton => buttonNorth;

        public ButtonControl this[GamepadButton button] => button switch
        {
            GamepadButton.DpadUp => Button("dpad/up", 11),
            GamepadButton.DpadDown => Button("dpad/down", 12),
            GamepadButton.DpadLeft => Button("dpad/left", 13),
            GamepadButton.DpadRight => Button("dpad/right", 14),
            GamepadButton.North => buttonNorth,
            GamepadButton.East => buttonEast,
            GamepadButton.South => buttonSouth,
            GamepadButton.West => buttonWest,
            GamepadButton.LeftStick => leftStickButton,
            GamepadButton.RightStick => rightStickButton,
            GamepadButton.LeftShoulder => leftShoulder,
            GamepadButton.RightShoulder => rightShoulder,
            GamepadButton.Start => startButton,
            GamepadButton.Select => selectButton,
            GamepadButton.LeftTrigger => leftTrigger,
            GamepadButton.RightTrigger => rightTrigger,
            _ => throw new ArgumentException($"no gamepad button {button}"),
        };

        /// <summary>Rumble: the low- and high-frequency motor, 0..1.</summary>
        public void SetMotorSpeeds(float lowFrequency, float highFrequency)
        {
            double* v = stackalloc double[16];
            v[0] = lowFrequency; v[1] = highFrequency;
            Internal.Native.api.inputSystem(96, (ulong)slot, 0, v, null, 0);
        }
        public void ResetHaptics() => SetMotorSpeeds(0f, 0f);
        public void PauseHaptics() => SetMotorSpeeds(0f, 0f);
    }

    /// <summary>Keyboard keys by their Input System names (`Keyboard.current[Key.Space]`).</summary>
    public enum Key
    {
        None,
        Space, Enter, Tab, Backquote, Quote, Semicolon, Comma, Period, Slash, Backslash,
        LeftBracket, RightBracket, Minus, Equals,
        A, B, C, D, E, F, G, H, I, J, K, L, M, N, O, P, Q, R, S, T, U, V, W, X, Y, Z,
        Digit1, Digit2, Digit3, Digit4, Digit5, Digit6, Digit7, Digit8, Digit9, Digit0,
        LeftShift, RightShift, LeftAlt, RightAlt, LeftCtrl, RightCtrl,
        Escape, LeftArrow, RightArrow, UpArrow, DownArrow,
        Backspace, PageDown, PageUp, Home, End, Insert, Delete,
        F1, F2, F3, F4, F5, F6, F7, F8, F9, F10, F11, F12,
    }

    /// <summary>The keyboard: `Keyboard.current.spaceKey.wasPressedThisFrame`.</summary>
    public sealed unsafe class Keyboard : InputDevice
    {
        static readonly Keyboard instance = new Keyboard();
        Keyboard() { }
        internal override string DeviceKey => "Keyboard";
        public static Keyboard current => instance;
        public override string name => "Keyboard";

        internal static string ControlName(Key key)
        {
            string s = key.ToString();
            if (s.StartsWith("Digit")) return s.Substring(5);
            if (s.Length == 1 || (s[0] == 'F' && char.IsDigit(s[1]))) return s.ToLowerInvariant();
            return char.ToLowerInvariant(s[0]) + s.Substring(1);
        }

        /// <summary>A key by its control name (`space`, `w`, `leftShift`, `1`, `f1`).</summary>
        internal ButtonControl Control(string control) =>
            new ButtonControl("/Keyboard/" + control, InputBridge.Control(0, control, out _));

        public ButtonControl this[Key key] => Control(ControlName(key));
        public ButtonControl anyKey => Control("anyKey");
        public ButtonControl shiftKey => Control("shift");
        public ButtonControl ctrlKey => Control("ctrl");
        public ButtonControl altKey => Control("alt");
        public ButtonControl spaceKey => Control("space");
        public ButtonControl enterKey => Control("enter");
        public ButtonControl tabKey => Control("tab");
        public ButtonControl escapeKey => Control("escape");
        public ButtonControl backspaceKey => Control("backspace");
        public ButtonControl deleteKey => Control("delete");
        public ButtonControl leftShiftKey => Control("leftShift");
        public ButtonControl rightShiftKey => Control("rightShift");
        public ButtonControl leftCtrlKey => Control("leftCtrl");
        public ButtonControl rightCtrlKey => Control("rightCtrl");
        public ButtonControl leftAltKey => Control("leftAlt");
        public ButtonControl rightAltKey => Control("rightAlt");
        public ButtonControl upArrowKey => Control("upArrow");
        public ButtonControl downArrowKey => Control("downArrow");
        public ButtonControl leftArrowKey => Control("leftArrow");
        public ButtonControl rightArrowKey => Control("rightArrow");
        public ButtonControl aKey => Control("a");
        public ButtonControl bKey => Control("b");
        public ButtonControl cKey => Control("c");
        public ButtonControl dKey => Control("d");
        public ButtonControl eKey => Control("e");
        public ButtonControl fKey => Control("f");
        public ButtonControl gKey => Control("g");
        public ButtonControl hKey => Control("h");
        public ButtonControl iKey => Control("i");
        public ButtonControl jKey => Control("j");
        public ButtonControl kKey => Control("k");
        public ButtonControl lKey => Control("l");
        public ButtonControl mKey => Control("m");
        public ButtonControl nKey => Control("n");
        public ButtonControl oKey => Control("o");
        public ButtonControl pKey => Control("p");
        public ButtonControl qKey => Control("q");
        public ButtonControl rKey => Control("r");
        public ButtonControl sKey => Control("s");
        public ButtonControl tKey => Control("t");
        public ButtonControl uKey => Control("u");
        public ButtonControl vKey => Control("v");
        public ButtonControl wKey => Control("w");
        public ButtonControl xKey => Control("x");
        public ButtonControl yKey => Control("y");
        public ButtonControl zKey => Control("z");
        public ButtonControl digit0Key => Control("0");
        public ButtonControl digit1Key => Control("1");
        public ButtonControl digit2Key => Control("2");
        public ButtonControl digit3Key => Control("3");
        public ButtonControl digit4Key => Control("4");
        public ButtonControl digit5Key => Control("5");
        public ButtonControl digit6Key => Control("6");
        public ButtonControl digit7Key => Control("7");
        public ButtonControl digit8Key => Control("8");
        public ButtonControl digit9Key => Control("9");
        public ButtonControl f1Key => Control("f1");
        public ButtonControl f2Key => Control("f2");
        public ButtonControl f3Key => Control("f3");
        public ButtonControl f4Key => Control("f4");
        public ButtonControl f5Key => Control("f5");
        public ButtonControl f6Key => Control("f6");
        public ButtonControl f7Key => Control("f7");
        public ButtonControl f8Key => Control("f8");
        public ButtonControl f9Key => Control("f9");
        public ButtonControl f10Key => Control("f10");
        public ButtonControl f11Key => Control("f11");
        public ButtonControl f12Key => Control("f12");
    }

    /// <summary>The mouse: `Mouse.current.position.ReadValue()` (pixels from the bottom-left),
    /// `delta`, `scroll`, `leftButton`.</summary>
    public sealed unsafe class Mouse : InputDevice
    {
        static readonly Mouse instance = new Mouse();
        Mouse() { }
        internal override string DeviceKey => "Mouse";
        public static Mouse current => instance;
        public override string name => "Mouse";

        ButtonControl Button(string control) =>
            new ButtonControl("/Mouse/" + control, InputBridge.Control(1, control, out _));
        Vector2Control Vector(string control)
        {
            InputBridge.Control(1, control, out var v);
            return new Vector2Control("/Mouse/" + control, v);
        }

        public Vector2Control position => Vector("position");
        /// <summary>Movement this frame in pixels, y up.</summary>
        public Vector2Control delta => Vector("delta");
        public Vector2Control scroll => Vector("scroll");
        public ButtonControl leftButton => Button("leftButton");
        public ButtonControl rightButton => Button("rightButton");
        public ButtonControl middleButton => Button("middleButton");
    }

    /// <summary>A finger of the touch screen (`Touchscreen.current.primaryTouch`).</summary>
    public sealed class TouchControl : InputControl
    {
        readonly Touch touch;
        readonly bool exists;
        internal TouchControl(string path, Touch touch, bool exists) : base(path) { this.touch = touch; this.exists = exists; }

        bool Down => exists && touch.phase != TouchPhase.Ended && touch.phase != TouchPhase.Canceled;
        public IntegerControl touchId => new IntegerControl(path + "/touchId", exists ? touch.fingerId : 0);
        public ButtonControl press => new ButtonControl(path + "/press", Down ? 1f : 0f, Down,
            exists && touch.phase == TouchPhase.Began, exists && !Down);
        public Vector2Control position => new Vector2Control(path + "/position", touch.position);
        public Vector2Control delta => new Vector2Control(path + "/delta", touch.deltaPosition);
        public TouchPhaseControl phase => new TouchPhaseControl(path + "/phase", touch.phase);
        public bool isInProgress => Down;
        /// <summary>Taps in a row (see `Touch.tapCount`).</summary>
        public IntegerControl tapCount => new IntegerControl(path + "/tapCount", exists ? touch.tapCount : 0);
    }

    /// <summary>The touch screen, or null until it has been touched.</summary>
    public sealed class Touchscreen : InputDevice
    {
        static readonly Touchscreen instance = new Touchscreen();
        Touchscreen() { }
        internal override string DeviceKey => "Touchscreen";
        public override string name => "Touchscreen";
        public static Touchscreen current => InputBridge.Call(12) == 1 ? instance : null;

        /// <summary>The first finger down (with nothing pressed when there is none).</summary>
        public TouchControl primaryTouch
        {
            get
            {
                var all = Input.touches;
                foreach (var t in all)
                    if (t.phase != TouchPhase.Ended && t.phase != TouchPhase.Canceled)
                        return new TouchControl("/Touchscreen/primaryTouch", t, true);
                return all.Length > 0
                    ? new TouchControl("/Touchscreen/primaryTouch", all[0], true)
                    : new TouchControl("/Touchscreen/primaryTouch", default, false);
            }
        }
        public TouchControl[] touches
        {
            get
            {
                var all = Input.touches;
                var list = new TouchControl[all.Length];
                for (int i = 0; i < all.Length; i++) list[i] = new TouchControl($"/Touchscreen/touch{i}", all[i], true);
                return list;
            }
        }
    }

    public enum InputActionType { Value, Button, PassThrough }
    public enum InputActionPhase { Disabled, Waiting, Started, Performed, Canceled }

    /// <summary>A control scheme of an Input Actions asset.</summary>
    public struct InputControlScheme
    {
        internal InputControlScheme(string name) { this.name = name; }
        public string name { get; }
        public override string ToString() => name;
    }

    /// <summary>An Input Actions asset being played (`playerInput.actions`, or
    /// `InputActionAsset.Load(path)`).</summary>
    public sealed unsafe class InputActionAsset
    {
        internal readonly int set;
        internal InputActionAsset(int set) { this.set = set; }

        static InputActionAsset Open(int op, string path)
        {
            long s = InputBridge.WithText(op, 0, 0, path);
            if (s < 0) throw new InvalidOperationException($"InputActionAsset {(op == 60 ? "Load" : "Instantiate")}: {InputBridge.LastError()}");
            return new InputActionAsset((int)s);
        }

        /// <summary>The asset at `path` (Assets/…inputactions), shared by everyone who loads it.</summary>
        public static InputActionAsset Load(string path) => Open(60, path);
        /// <summary>A copy of the asset at `path` of its own (its own enabled state and values).</summary>
        public static InputActionAsset Instantiate(string path) => Open(61, path);

        public string name => InputBridge.Text(62, (ulong)set, 0);
        public bool enabled => InputBridge.Call(79, (ulong)set) == 1;
        public InputActionMap[] actionMaps
        {
            get
            {
                int n = (int)InputBridge.Check(InputBridge.Call(75, (ulong)set));
                var maps = new InputActionMap[n];
                for (int i = 0; i < n; i++) maps[i] = new InputActionMap(set, i);
                return maps;
            }
        }
        public InputControlScheme[] controlSchemes
        {
            get
            {
                string all = InputBridge.Text(80, (ulong)set, 0);
                if (all.Length == 0) return Array.Empty<InputControlScheme>();
                return Array.ConvertAll(all.Split('\n'), n => new InputControlScheme(n));
            }
        }

        /// <summary>An action by name ("Jump") or "Map/Action"; null (or an exception) when none.</summary>
        public InputAction FindAction(string actionNameOrId, bool throwIfNotFound = false)
        {
            long packed = InputBridge.WithText(63, (ulong)set, 0, actionNameOrId);
            if (packed >= 0) return new InputAction(set, (int)(packed >> 32), (int)(packed & 0xffffffff));
            if (throwIfNotFound) throw new ArgumentException($"No action '{actionNameOrId}' in '{name}'");
            return null;
        }

        public InputActionMap FindActionMap(string nameOrId, bool throwIfNotFound = false)
        {
            long map = InputBridge.WithText(64, (ulong)set, 0, nameOrId);
            if (map >= 0) return new InputActionMap(set, (int)map);
            if (throwIfNotFound) throw new ArgumentException($"No action map '{nameOrId}' in '{name}'");
            return null;
        }

        public InputAction this[string actionNameOrId] =>
            FindAction(actionNameOrId) ?? throw new KeyNotFoundException($"Cannot find action '{actionNameOrId}'");

        public void Enable() => InputBridge.Check(InputBridge.Call(65, (ulong)set, 1));
        public void Disable() => InputBridge.Check(InputBridge.Call(65, (ulong)set, 0));

        /// <summary>The binding overrides (rebinds) as JSON, for PlayerPrefs.</summary>
        public string SaveBindingOverridesAsJson() => InputBridge.Text(81, (ulong)set, 0);
        public void LoadBindingOverridesFromJson(string json, bool removeExisting = true)
        {
            if (removeExisting) RemoveAllBindingOverrides();
            if (InputBridge.WithText(82, (ulong)set, 0, json) < 0)
                throw new ArgumentException(InputBridge.LastError());
        }
        public void RemoveAllBindingOverrides()
        {
            double* v = stackalloc double[16];
            InputBridge.Check(Internal.Native.api.inputSystem(83, (ulong)set, -1, v, null, 0));
        }

        public override bool Equals(object obj) => obj is InputActionAsset a && a.set == set;
        public override int GetHashCode() => set;
        public override string ToString() => name;
    }

    /// <summary>An action map of an Input Actions asset ("Player", "UI").</summary>
    public sealed unsafe class InputActionMap
    {
        internal readonly int set, map;
        internal InputActionMap(int set, int map) { this.set = set; this.map = map; }
        long Packed => (long)map << 32;

        public string name => InputBridge.Text(66, (ulong)set, Packed);
        public bool enabled => InputBridge.Check(InputBridge.Call(67, (ulong)set, Packed)) == 1;
        public InputActionAsset asset => new InputActionAsset(set);
        public InputAction[] actions
        {
            get
            {
                int n = (int)InputBridge.Check(InputBridge.Call(76, (ulong)set, Packed));
                var list = new InputAction[n];
                for (int i = 0; i < n; i++) list[i] = new InputAction(set, map, i);
                return list;
            }
        }

        public InputAction FindAction(string nameOrId, bool throwIfNotFound = false)
        {
            long a = InputBridge.WithText(69, (ulong)set, Packed, nameOrId);
            if (a >= 0) return new InputAction(set, map, (int)a);
            if (throwIfNotFound) throw new ArgumentException($"No action '{nameOrId}' in map '{name}'");
            return null;
        }

        public InputAction this[string actionNameOrId] =>
            FindAction(actionNameOrId) ?? throw new KeyNotFoundException($"Cannot find action '{actionNameOrId}'");

        void SetEnabled(bool on)
        {
            double* v = stackalloc double[16];
            v[0] = on ? 1 : 0;
            InputBridge.Check(Internal.Native.api.inputSystem(68, (ulong)set, Packed, v, null, 0));
        }
        public void Enable() => SetEnabled(true);
        public void Disable() => SetEnabled(false);

        public override bool Equals(object obj) => obj is InputActionMap m && m.set == set && m.map == map;
        public override int GetHashCode() => HashCode.Combine(set, map);
        public override string ToString() => name;
    }

    /// <summary>An action ("Move", "Jump"): poll it (`ReadValue&lt;Vector2&gt;()`,
    /// `WasPressedThisFrame()`) or listen (`performed += ctx => …`).</summary>
    public sealed unsafe class InputAction
    {
        internal readonly int set, map, action;
        internal InputAction(int set, int map, int action) { this.set = set; this.map = map; this.action = action; }
        long Packed => ((long)map << 32) | (long)(uint)action;
        /// <summary>The key C# keeps this action's listeners under (`action_key` in csharp_input.rs).</summary>
        internal ulong EventKey => (1UL << 63) | ((ulong)(uint)set << 40) | ((ulong)(uint)map << 20) | (ulong)(uint)action;

        public string name => InputBridge.Text(70, (ulong)set, Packed);
        public InputActionMap actionMap => new InputActionMap(set, map);
        public InputActionType type => (InputActionType)InputBridge.Check(InputBridge.Call(74, (ulong)set, Packed));

        long State => InputBridge.Check(InputBridge.Call(72, (ulong)set, Packed));
        public InputActionPhase phase => (InputActionPhase)((State >> 8) & 0xff);
        public bool enabled => (State & 16) != 0;
        /// <summary>Performed this frame.</summary>
        public bool triggered => (State & 8) != 0;
        /// <summary>The path of the control driving the action ("" when none).</summary>
        public string activeControlPath => InputBridge.Text(78, (ulong)set, Packed);

        public bool IsPressed() => (State & 1) != 0;
        public bool WasPressedThisFrame() => (State & 2) != 0;
        public bool WasReleasedThisFrame() => (State & 4) != 0;
        public bool WasPerformedThisFrame() => (State & 8) != 0;
        public bool IsInProgress() => (State & 32) != 0;

        /// <summary>The value now: float for buttons and axes, Vector2 for sticks and 2D
        /// composites (also bool, int, double, Vector3).</summary>
        public TValue ReadValue<TValue>() where TValue : struct
        {
            double* v = stackalloc double[16];
            long vector = InputBridge.Check(Internal.Native.api.inputSystem(71, (ulong)set, Packed, v, null, 0));
            return InputBridge.Convert<TValue>(new Vector2((float)v[0], (float)v[1]), vector == 1, (float)v[2], name);
        }

        public object ReadValueAsObject()
        {
            double* v = stackalloc double[16];
            long vector = InputBridge.Check(Internal.Native.api.inputSystem(71, (ulong)set, Packed, v, null, 0));
            return vector == 1 ? new Vector2((float)v[0], (float)v[1]) : (object)(float)v[0];
        }

        /// <summary>How far the control is actuated, 0..1 (a stick's length).</summary>
        public float GetControlMagnitude()
        {
            double* v = stackalloc double[16];
            InputBridge.Check(Internal.Native.api.inputSystem(71, (ulong)set, Packed, v, null, 0));
            return (float)v[2];
        }

        void SetEnabled(bool on)
        {
            double* v = stackalloc double[16];
            v[0] = on ? 1 : 0;
            InputBridge.Check(Internal.Native.api.inputSystem(73, (ulong)set, Packed, v, null, 0));
        }
        public void Enable() => SetEnabled(true);
        public void Disable() => SetEnabled(false);

        /// <summary>The bindings as shown to players ("Space", "W/A/S/D"); `bindingIndex` -1 is
        /// all of them.</summary>
        public string GetBindingDisplayString(int bindingIndex = -1)
        {
            double* v = stackalloc double[16];
            v[0] = bindingIndex;
            return InputBridge.Text(77, (ulong)set, Packed, v);
        }

        /// <summary>Rebind binding `bindingIndex` to `path` ("&lt;Keyboard&gt;/space").</summary>
        public void ApplyBindingOverride(int bindingIndex, string path)
        {
            double* v = stackalloc double[16];
            v[0] = bindingIndex;
            if (InputBridge.WithText(84, (ulong)set, Packed, path, v) < 0)
                throw new ArgumentOutOfRangeException(nameof(bindingIndex), InputBridge.LastError());
        }
        public void ApplyBindingOverride(string path) => ApplyBindingOverride(0, path);
        public void RemoveBindingOverride(int bindingIndex) => ApplyBindingOverride(bindingIndex, "");
        public void RemoveAllBindingOverrides()
        {
            double* v = stackalloc double[16];
            v[0] = 1;
            InputBridge.Check(Internal.Native.api.inputSystem(83, (ulong)set, Packed, v, null, 0));
        }

        /// <summary>The bindings, numbered like `GetBindingDisplayString` and
        /// `ApplyBindingOverride` count them (a composite, then its parts).</summary>
        public InputBinding[] bindings => InputBinding.Of(this);

        /// <summary>The index of the first binding in control scheme group `group` and / or
        /// with (effective) path `path`, skipping composites themselves; -1 when none.</summary>
        public int GetBindingIndex(string group = null, string path = null)
        {
            var list = bindings;
            for (int i = 0; i < list.Length; i++)
                if (!list[i].isComposite && list[i].Matches(group, path)) return i;
            return -1;
        }

        /// <summary>Listen for the next control pressed and bind binding `bindingIndex` to it
        /// (-1: the first binding in the control scheme in use). Call `Start()` on the result;
        /// the action is disabled until the operation ends.</summary>
        public InputActionRebindingExtensions.RebindingOperation PerformInteractiveRebinding(int bindingIndex = -1) =>
            InputActionRebindingExtensions.RebindingOperation.Create(this, bindingIndex);

        /// <summary>Phase changes, as C# events (`action.performed += OnJump`).</summary>
        public event Action<CallbackContext> started
        {
            add => InputEvents.Add(EventKey, 110, value);
            remove => InputEvents.Remove(EventKey, 110, value);
        }
        public event Action<CallbackContext> performed
        {
            add => InputEvents.Add(EventKey, 111, value);
            remove => InputEvents.Remove(EventKey, 111, value);
        }
        public event Action<CallbackContext> canceled
        {
            add => InputEvents.Add(EventKey, 112, value);
            remove => InputEvents.Remove(EventKey, 112, value);
        }

        public static bool operator ==(InputAction a, InputAction b) => a is null ? b is null : a.Equals(b);
        public static bool operator !=(InputAction a, InputAction b) => !(a == b);
        public override bool Equals(object obj) => obj is InputAction a && a.set == set && a.map == map && a.action == action;
        public override int GetHashCode() => HashCode.Combine(set, map, action);
        public override string ToString() => $"{actionMap.name}/{name}";

        /// <summary>What a listener (or an Invoke Events method) gets about a phase change.</summary>
        public struct CallbackContext
        {
            readonly Vector2 value;
            readonly bool vector;
            readonly float magnitude;
            readonly string controlPath;
            internal CallbackContext(InputAction action, InputActionPhase phase, Vector2 value, bool vector,
                float magnitude, double time, double startTime, string controlPath, string interaction)
            {
                this.action = action; this.phase = phase; this.value = value; this.vector = vector;
                this.magnitude = magnitude; this.time = time; this.startTime = startTime;
                this.controlPath = controlPath ?? ""; this.interactionName = interaction ?? "";
            }

            public InputAction action { get; }
            public InputActionPhase phase { get; }
            public bool started => phase == InputActionPhase.Started;
            public bool performed => phase == InputActionPhase.Performed;
            public bool canceled => phase == InputActionPhase.Canceled;
            /// <summary>Real seconds since Play when it happened.</summary>
            public double time { get; }
            public double startTime { get; }
            public double duration => time - startTime;
            /// <summary>The control that drove it.</summary>
            public InputControl control => new InputControl(controlPath);
            /// <summary>The interaction that drove it ("Hold", "Tap"; "" for the default).</summary>
            public string interactionName { get; }

            public TValue ReadValue<TValue>() where TValue : struct =>
                InputBridge.Convert<TValue>(value, vector, magnitude, action?.name ?? "action");
            public object ReadValueAsObject() => vector ? value : (object)value.x;
            public bool ReadValueAsButton() => magnitude >= 0.5f;
            public override string ToString() => $"{{ action={action} phase={phase} }}";
        }
    }

    /// <summary>The value a Player Input message carries (`void OnMove(InputValue value)`).</summary>
    public sealed class InputValue
    {
        readonly Vector2 value;
        readonly bool vector;
        internal InputValue(Vector2 value, bool vector) { this.value = value; this.vector = vector; }
        public TValue Get<TValue>() where TValue : struct =>
            InputBridge.Convert<TValue>(value, vector, vector ? value.magnitude : Math.Abs(value.x), "InputValue");
        public object Get() => vector ? value : (object)value.x;
        public bool isPressed => (vector ? value.magnitude : Math.Abs(value.x)) >= 0.5f;
        public override string ToString() => vector ? value.ToString() : value.x.ToString(System.Globalization.CultureInfo.InvariantCulture);
    }

    /// <summary>How Player Input reports actions.</summary>
    public enum PlayerNotifications { SendMessages, BroadcastMessages, InvokeUnityEvents, InvokeCSharpEvents }

    /// <summary>When the Player Input Manager lets players join.</summary>
    public enum PlayerJoinBehavior { JoinPlayersWhenButtonIsPressed, JoinPlayersManually }

    /// <summary>The object's Player Input: one player's actions, devices and camera.</summary>
    public sealed unsafe class PlayerInput : Component
    {
        internal readonly ulong id;
        internal PlayerInput(ulong id) { this.id = id; }
        internal override ulong EntityId => id;
        internal override bool Same(Object other) => other is PlayerInput p && p.id == id;

        /// <summary>0-based in joining (Hierarchy) order; -1 while not playing.</summary>
        public int playerIndex => (int)InputBridge.Call(41, id);
        /// <summary>The player's camera: the first Camera on the object or under it (split
        /// screen gives it its part of the screen).</summary>
        public Camera camera
        {
            get { long c = InputBridge.Call(42, id); return c <= 0 ? null : new Camera((ulong)c); }
        }
        /// <summary>The Input Actions this player plays (its own copy).</summary>
        public InputActionAsset actions => new InputActionAsset((int)InputBridge.Check(InputBridge.Call(45, id)));
        public string currentControlScheme => InputBridge.Text(44, id, 0);
        public InputActionMap currentActionMap
        {
            get
            {
                long set = InputBridge.Check(InputBridge.Call(45, id));
                long map = InputBridge.Call(46, id);
                return map < 0 ? null : new InputActionMap((int)set, (int)map);
            }
            set => SwitchCurrentActionMap(value?.name ?? "");
        }
        public bool inputIsActive => InputBridge.Check(InputBridge.Call(47, id)) == 1;
        public PlayerNotifications notificationBehavior => (PlayerNotifications)InputBridge.Check(InputBridge.Call(55, id));

        /// <summary>The devices paired with this player.</summary>
        public InputDevice[] devices
        {
            get
            {
                string all = InputBridge.Text(43, id, 0);
                var list = new List<InputDevice>();
                foreach (var n in all.Split('\n', StringSplitOptions.RemoveEmptyEntries))
                {
                    if (n == "Keyboard") list.Add(Keyboard.current);
                    else if (n == "Mouse") list.Add(Mouse.current);
                    else if (n == "Touchscreen") list.Add(Touchscreen.current ?? (InputDevice)Keyboard.current);
                    else if (n.StartsWith("Gamepad "))
                    {
                        var digits = n.Substring(8).Split(' ')[0];
                        if (int.TryParse(digits, out int k)) list.Add(new Gamepad(k - 1));
                    }
                }
                return list.ToArray();
            }
        }

        /// <summary>The player's first device of type TDevice (`GetDevice&lt;Gamepad&gt;()`), or null.</summary>
        public TDevice GetDevice<TDevice>() where TDevice : InputDevice
        {
            if (typeof(TDevice) == typeof(Gamepad))
            {
                long s = InputBridge.Call(56, id);
                return s < 0 ? null : new Gamepad((int)s) as TDevice;
            }
            foreach (var d in devices) if (d is TDevice t) return t;
            return null;
        }

        public void SwitchCurrentActionMap(string mapNameOrId)
        {
            if (InputBridge.WithText(48, id, 0, mapNameOrId) < 0)
                throw new ArgumentException(InputBridge.LastError());
        }
        public void SwitchCurrentControlScheme(string controlScheme, params InputDevice[] devices)
        {
            if (InputBridge.WithText(49, id, 0, controlScheme) < 0)
                throw new ArgumentException(InputBridge.LastError());
        }
        public void ActivateInput() => InputBridge.Call(50, id);
        public void DeactivateInput() => InputBridge.Call(51, id);

        /// <summary>Every action of this player (Invoke C# Events style).</summary>
        public event Action<InputAction.CallbackContext> onActionTriggered
        {
            add => InputEvents.Add(id, 120, value);
            remove => InputEvents.Remove(id, 120, value);
        }
        public event Action<PlayerInput> onControlsChanged
        {
            add => InputEvents.Add(id, 121, value);
            remove => InputEvents.Remove(id, 121, value);
        }
        public event Action<PlayerInput> onDeviceLost
        {
            add => InputEvents.Add(id, 122, value);
            remove => InputEvents.Remove(id, 122, value);
        }
        public event Action<PlayerInput> onDeviceRegained
        {
            add => InputEvents.Add(id, 123, value);
            remove => InputEvents.Remove(id, 123, value);
        }

        /// <summary>Every playing Player Input, in player order.</summary>
        public static PlayerInput[] all
        {
            get
            {
                int n = (int)InputBridge.Call(52);
                var list = new List<PlayerInput>();
                for (int i = 0; i < n; i++)
                {
                    long e = InputBridge.Call(53, 0, i);
                    if (e > 0) list.Add(new PlayerInput((ulong)e));
                }
                list.Sort((a, b) => a.playerIndex.CompareTo(b.playerIndex));
                return list.ToArray();
            }
        }

        public static PlayerInput GetPlayerByIndex(int playerIndex)
        {
            long e = InputBridge.Call(54, 0, playerIndex);
            return e > 0 ? new PlayerInput((ulong)e) : null;
        }
    }

    /// <summary>The Player Input Manager: local multiplayer. A button pressed on a device no
    /// player uses spawns the Player Prefab paired with it; with splitScreen on each player's
    /// camera gets its part of the screen. Its scripts hear `OnPlayerJoined(PlayerInput)` and
    /// `OnPlayerLeft(PlayerInput)`.</summary>
    public sealed class PlayerInputManager : Component
    {
        internal readonly ulong id;
        internal PlayerInputManager(ulong id) { this.id = id; }
        internal override ulong EntityId => id;
        internal override bool Same(Object other) => other is PlayerInputManager m && m.id == id;

        /// <summary>The scene's running Player Input Manager, or null.</summary>
        public static PlayerInputManager instance
        {
            get { long e = InputBridge.Call(20); return e > 0 ? new PlayerInputManager((ulong)e) : null; }
        }

        long Get(int op) => InputBridge.Check(InputBridge.Call(op, id));
        void Set(int op, bool on) => InputBridge.Check(InputBridge.Call(op, id, on ? 1 : 0));

        /// <summary>Players in the scene (active Player Input objects).</summary>
        public int playerCount => (int)InputBridge.Call(21);
        /// <summary>Most players at once; -1 for no limit.</summary>
        public int maxPlayerCount { get { int n = (int)Get(22); return n == 0 ? -1 : n; } }
        public bool joiningEnabled => Get(23) == 1;
        public bool splitScreen { get => Get(25) == 1; set => Set(26, value); }
        public bool enabled { get => Get(27) == 1; set => Set(28, value); }
        public PlayerJoinBehavior joinBehavior
        {
            get => (PlayerJoinBehavior)Get(29);
            set => Set(30, value == PlayerJoinBehavior.JoinPlayersManually);
        }
        /// <summary>Split into at least this many parts even with fewer players (-1: as many as
        /// players).</summary>
        public int fixedNumberOfSplitScreens { get { int n = (int)Get(31); return n == 0 ? -1 : n; } }

        public void EnableJoining() => Set(24, true);
        public void DisableJoining() => Set(24, false);

        /// <summary>Spawn the Player Prefab now (paired with free devices): its Player Input,
        /// or null when the manager is full. Its scripts get Awake right after the calling
        /// method; the manager hears OnPlayerJoined at the next frame.</summary>
        public PlayerInput JoinPlayer()
        {
            long e = InputBridge.Call(32, id);
            if (e < 0) throw new InvalidOperationException(InputBridge.LastError());
            return e == 0 ? null : new PlayerInput((ulong)e);
        }

        public event Action<PlayerInput> onPlayerJoined
        {
            add => InputEvents.Add(id, 130, value);
            remove => InputEvents.Remove(id, 130, value);
        }
        public event Action<PlayerInput> onPlayerLeft
        {
            add => InputEvents.Add(id, 131, value);
            remove => InputEvents.Remove(id, 131, value);
        }
    }

    /// <summary>The engine side of the input classes (`csharp_input.rs`).</summary>
    internal static unsafe class InputBridge
    {
        internal static long Call(int op, ulong id = 0, long arg = 0) =>
            Internal.Native.api.inputSystem(op, id, arg, null, null, 0);

        internal static long WithText(int op, ulong id, long arg, string text, double* v = null)
        {
            text ??= "";
            fixed (char* p = text) return Internal.Native.api.inputSystem(op, id, arg, v, p, text.Length);
        }

        /// <summary>A text result (or an exception).</summary>
        internal static string Text(int op, ulong id, long arg, double* v = null)
        {
            char* buf = stackalloc char[256];
            long n = Internal.Native.api.inputSystem(op, id, arg, v, buf, 256);
            if (n < 0) throw new InvalidOperationException(LastError());
            if (n <= 256) return new string(buf, 0, (int)n);
            var big = new char[n];
            fixed (char* p = big)
            {
                n = Internal.Native.api.inputSystem(op, id, arg, v, p, big.Length);
                return new string(p, 0, (int)Math.Clamp(n, 0, big.Length));
            }
        }

        internal static string LastError()
        {
            char* buf = stackalloc char[512];
            long n = Internal.Native.api.inputSystem(199, 0, 0, null, buf, 512);
            return new string(buf, 0, (int)Math.Clamp(n, 0, 512));
        }

        internal static long Check(long r) => r == -1 ? throw new InvalidOperationException(LastError()) : r;

        internal static ulong Id(GameObject go) => go != null ? go.ObjectId : 0;
        internal static GameObject Wrap(long id) => id == 0 ? null : new GameObject((ulong)id);

        /// <summary>A Keyboard (0) or Mouse (1) control: its button bits, and its value in `value`.</summary>
        internal static long Control(int device, string control, out Vector2 value)
        {
            double* v = stackalloc double[16];
            long bits = Check(WithText(100, 0, device, control, v));
            value = new Vector2((float)v[0], (float)v[1]);
            return bits;
        }

        internal static Touch GetTouch(int index)
        {
            double* v = stackalloc double[16];
            if (Internal.Native.api.inputSystem(11, 0, index, v, null, 0) != 1)
                throw new ArgumentException($"Index out of bounds: no touch {index}", nameof(index));
            var position = new Vector2((float)v[1], (float)v[2]);
            return new Touch
            {
                fingerId = (int)v[0],
                position = position,
                rawPosition = position,
                deltaPosition = new Vector2((float)v[3], (float)v[4]),
                phase = (TouchPhase)(int)v[5],
                tapCount = (int)v[6],
            };
        }

        internal static Touch[] Touches()
        {
            int n = (int)Call(10);
            var list = new Touch[Math.Max(n, 0)];
            for (int i = 0; i < list.Length; i++) list[i] = GetTouch(i);
            return list;
        }

        internal static Rect CameraRect(ulong camera, int op)
        {
            double* v = stackalloc double[16];
            Check(Internal.Native.api.inputSystem(op, camera, 0, v, null, 0));
            return new Rect((float)v[0], (float)v[1], (float)v[2], (float)v[3]);
        }

        internal static void SetCameraRect(ulong camera, Rect r)
        {
            double* v = stackalloc double[16];
            v[0] = r.x; v[1] = r.y; v[2] = r.width; v[3] = r.height;
            Check(Internal.Native.api.inputSystem(16, camera, 0, v, null, 0));
        }

        /// <summary>An action value as `TValue` (float, Vector2, bool, int, double, Vector3).</summary>
        internal static TValue Convert<TValue>(Vector2 v, bool vector, float magnitude, string what) where TValue : struct
        {
            var t = typeof(TValue);
            if (t == typeof(Vector2)) return (TValue)(object)v;
            if (t == typeof(Vector3)) return (TValue)(object)new Vector3(v.x, v.y, 0f);
            float f = vector ? magnitude : v.x;
            if (t == typeof(float)) return (TValue)(object)f;
            if (t == typeof(double)) return (TValue)(object)(double)f;
            if (t == typeof(int)) return (TValue)(object)(int)Math.Round(f);
            if (t == typeof(bool)) return (TValue)(object)(Math.Abs(f) >= 0.5f);
            throw new InvalidOperationException($"Cannot read value of type '{t.Name}' from '{what}' (it is a {(vector ? "Vector2" : "float")})");
        }

        /// <summary>The argument of the engine's running call into C# (`CsArg::Engine`).</summary>
        internal static object TakeArg()
        {
            double* v = stackalloc double[16];
            char* buf = stackalloc char[512];
            long kind = Internal.Native.api.inputSystem(120, 0, 0, v, buf, 512);
            switch (kind)
            {
                case 1: return new PlayerInput(*(ulong*)v);
                case 2: return new InputValue(new Vector2((float)v[0], (float)v[1]), v[2] != 0);
                case 3:
                {
                    string text = new string(buf, 0, (int)Math.Clamp(v[15], 0, 512));
                    int nl = text.IndexOf('\n');
                    string control = nl < 0 ? text : text.Substring(0, nl);
                    string interaction = nl < 0 ? "" : text.Substring(nl + 1);
                    var action = new InputAction((int)v[0], (int)v[1], (int)v[2]);
                    return new InputAction.CallbackContext(action, (InputActionPhase)(int)v[3],
                        new Vector2((float)v[4], (float)v[5]), v[6] != 0, (float)v[9], v[7], v[8], control, interaction);
                }
                case 4: return new BaseEventData();
                case 5: return new string(buf, 0, (int)Math.Clamp(v[15], 0, 512));
                default: return null;
            }
        }

        /// <summary>`GetComponent&lt;PlayerInput&gt;()` / `&lt;PlayerInputManager&gt;()`, or null.</summary>
        internal static Component Component(Type t, ulong entity)
        {
            if (t == typeof(PlayerInput))
                return Call(40, entity) == 1 ? new PlayerInput(entity) : null;
            if (t == typeof(PlayerInputManager))
                return Call(33, entity) == 1 ? new PlayerInputManager(entity) : null;
            return null;
        }
    }

    /// <summary>C# listeners of input events (`action.performed += …`, `onPlayerJoined`), by
    /// key (an action or an object) and kind (110 started, 111 performed, 112 canceled, 120
    /// onActionTriggered, 121 onControlsChanged, 122 onDeviceLost, 123 onDeviceRegained, 130
    /// onPlayerJoined, 131 onPlayerLeft; the `EV_` constants in csharp_input.rs).</summary>
    internal static class InputEvents
    {
        static readonly Dictionary<(ulong, int), List<Delegate>> listeners = new Dictionary<(ulong, int), List<Delegate>>();

        internal static void Reset() => listeners.Clear();

        internal static void Add(ulong key, int kind, Delegate call)
        {
            if (call == null) return;
            if (!listeners.TryGetValue((key, kind), out var list)) listeners[(key, kind)] = list = new List<Delegate>();
            list.Add(call);
        }

        internal static void Remove(ulong key, int kind, Delegate call)
        {
            if (call != null && listeners.TryGetValue((key, kind), out var list)) list.Remove(call);
        }

        /// <summary>Run the listeners; whether there were any.</summary>
        internal static bool Run(ulong key, int kind, object arg)
        {
            if (!listeners.TryGetValue((key, kind), out var list) || list.Count == 0) return false;
            foreach (var call in list.ToArray())
            {
                try
                {
                    switch (call)
                    {
                        case Action<InputAction.CallbackContext> c when arg is InputAction.CallbackContext ctx: c(ctx); break;
                        case Action<PlayerInput> p: p(arg as PlayerInput); break;
                        default: call.DynamicInvoke(arg); break;
                    }
                }
                catch (Exception ex)
                {
                    Internal.Host.Report(call.Method.DeclaringType?.Name ?? "Input", "listener", ex);
                }
            }
            return true;
        }
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
        public delegate* unmanaged<int, long, char*, int, int, long> request;
        public delegate* unmanaged<ulong, int, int, int, Color*, int> componentValue;
        public delegate* unmanaged<ulong, int, Vector3*, Vector3*, Vector3*, int> cameraPoint;
        public delegate* unmanaged<ulong, int, int, char*, int, Color*, int> material;
        public delegate* unmanaged<int, int, Color*, ulong> global;
        public delegate* unmanaged<ulong, int, int, char*, int, int> componentText;
        public delegate* unmanaged<int, char*, int, double*, char*, int, int> prefs;
        public delegate* unmanaged<ulong, int, Vector3*, int, int> rigidbody2d;
        public delegate* unmanaged<ulong, int, Vector3*, int> navAgent;
        public delegate* unmanaged<ulong, int, float*, int> particles;
        public delegate* unmanaged<ulong, int, char*, int, int> componentOp;
        public delegate* unmanaged<long, int, int> sceneOp;
        public delegate* unmanaged<ulong, char*, int, int, double, char*, int, int> message;
        public delegate* unmanaged<int, Vector3*, Vector3*, float, uint, HitData*, ulong*, int, int> physics2d;
        public delegate* unmanaged<int, ulong, long, double*, char*, int, long> inputSystem;
        // ---- csharp_parity.rs ----
        public delegate* unmanaged<int, float*, int, ulong*, int> contacts;
        public delegate* unmanaged<ulong, int, int, float*, int> animatorIk;
        public delegate* unmanaged<ulong, int, Vector3*, int> characterVec;
        public delegate* unmanaged<ulong, int, int, float, float*, int> characterScalar;
        public delegate* unmanaged<float, void> timeScale;
        public delegate* unmanaged<int, ulong*, int, float*, int> transformGroup;
        public delegate* unmanaged<int, int, char*, int, char*, int, int> app;
        public delegate* unmanaged<int, int, float*, uint, HitData*, ulong*, int, int> shapeQuery;
        public delegate* unmanaged<int, ulong, ulong, Vector3*, float, float, float, int> physicsExtra;
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

    /// <summary>C# listeners of UI events, by object and event (0 onClick, 1 onValueChanged,
    /// 2 onEndEdit; `UiEventKind` in csharp.rs).</summary>
    internal static class UiEvents
    {
        static readonly Dictionary<(ulong, int), List<Delegate>> listeners = new Dictionary<(ulong, int), List<Delegate>>();

        internal static void Reset() => listeners.Clear();

        internal static void Add(ulong entity, int kind, Delegate call)
        {
            if (call == null) return;
            if (!listeners.TryGetValue((entity, kind), out var list)) listeners[(entity, kind)] = list = new List<Delegate>();
            list.Add(call);
        }

        internal static void Remove(ulong entity, int kind, Delegate call)
        {
            if (listeners.TryGetValue((entity, kind), out var list)) list.Remove(call);
        }

        internal static void Clear(ulong entity, int kind) => listeners.Remove((entity, kind));

        /// <summary>Run the listeners; whether there were any.</summary>
        internal static bool Run(ulong entity, int kind, object value)
        {
            if (!listeners.TryGetValue((entity, kind), out var list) || list.Count == 0) return false;
            foreach (var call in list.ToArray())
            {
                try
                {
                    switch (call)
                    {
                        case Action a: a(); break;
                        case Action<bool> b: b(value is bool v ? v : Convert.ToDouble(value ?? 0) != 0); break;
                        case Action<float> f: f(Convert.ToSingle(value ?? 0f)); break;
                        case Action<int> i: i(Convert.ToInt32(value ?? 0)); break;
                        case Action<string> t: t(Convert.ToString(value, System.Globalization.CultureInfo.InvariantCulture) ?? ""); break;
                        default: call.DynamicInvoke(value); break;
                    }
                }
                catch (Exception ex)
                {
                    Host.Report(call.Method.DeclaringType?.Name ?? "UI", "listener", ex);
                }
            }
            return true;
        }
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

        /// <summary>which: 0 add, 1 remove. 1 when it changed, 0 when not, -1 for an unknown name.</summary>
        /// <summary>Queue SendMessage for the Lua scripts of `id`; whether it has any.</summary>
        internal static bool QueueMessage(ulong id, string method, object value)
        {
            int kind = -1;
            double num = 0;
            string text = "";
            switch (value)
            {
                case null: break;
                case bool b: kind = 0; num = b ? 1 : 0; break;
                case int or long or short or byte or uint or ulong: kind = 2; num = Convert.ToDouble(value); break;
                case float or double or decimal: kind = 1; num = Convert.ToDouble(value); break;
                case string str: kind = 3; text = str; break;
                default: kind = -2; break;
            }
            fixed (char* m = method)
            fixed (char* t = text)
                return api.message(id, m, method.Length, kind, num, t, text.Length) != 0;
        }

        internal static int ComponentOp(ulong id, int op, string name)
        {
            fixed (char* p = name) return api.componentOp(id, op, p, name.Length);
        }

        internal static Component AddComponent(ulong id, Type type)
        {
            if (!Alive(id)) throw Destroyed();
            if (type == typeof(Transform)) return new Transform(id);
            if (typeof(MonoBehaviour).IsAssignableFrom(type))
            {
                if (type.IsAbstract || type == typeof(MonoBehaviour))
                    throw new ArgumentException($"AddComponent<{type.Name}>: name a concrete script class.");
                int slot = ComponentOp(id, 3, type.Name);
                return slot < 0 ? null : Host.PreCreate(type, id, slot);
            }
            if (type == typeof(Collider) || type == typeof(Collider2D) || type == typeof(Behaviour) || type == typeof(UIBehaviour) || type == typeof(Component))
                throw new ArgumentException($"AddComponent<{type.Name}>: name a concrete type, e.g. BoxCollider.");
            string name = type == typeof(MeshRenderer) || type == typeof(Renderer) ? "MeshRenderer" : type.Name;
            if (ComponentOp(id, 0, name) < 0) throw new ArgumentException($"AddComponent<{type.Name}>: not an engine component.");
            var found = Host.EngineComponentOf(type, id);
            return found ?? new GameObject(id).GetComponent<Component>();
        }

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

        /// <summary>One TransformGroup call for `count` objects (ops in csharp_native.rs).</summary>
        internal static int TransformGroup(int op, ulong[] ids, int count, float* data)
        {
            if (count <= 0) return 0;
            fixed (ulong* p = ids)
            {
                int done = api.transformGroup(op, p, count, data);
                if (done < 0) throw new InvalidOperationException("TransformGroup used outside a script callback");
                return done;
            }
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
            "OnCollisionEnter2D", "OnCollisionExit2D", "OnTriggerEnter2D", "OnTriggerExit2D",
            "OnCollisionStay2D", "OnTriggerStay2D",
            "OnAnimatorIK", "OnControllerColliderHit",
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
            UiEvents.Reset();
            InputEvents.Reset();
            AsyncOperation.Reset();
            types.Clear();
            byObject.Clear();
            preCreated.Clear();
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
                // AddComponent made the object already (it was returned to the caller).
                if (!preCreated.Remove((entity, slot), out var script) || script.GetType() != type)
                    script = Construct(type, entity, slot);
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

        /// <summary>Scripts made by AddComponent, waiting for the engine to create their
        /// instance right after the calling callback (Awake, OnEnable).</summary>
        static readonly Dictionary<(ulong, int), MonoBehaviour> preCreated = new Dictionary<(ulong, int), MonoBehaviour>();

        static MonoBehaviour Construct(Type type, ulong entity, int slot)
        {
            var script = (MonoBehaviour)RuntimeHelpers_GetUninitialized(type);
            script.entity = entity;
            script.slot = slot;
            // Field initialisers and the constructor run with the object already known.
            type.GetConstructor(BindingFlags.Instance | BindingFlags.Public | BindingFlags.NonPublic, null, Type.EmptyTypes, null)
                ?.Invoke(script, null);
            return script;
        }

        internal static MonoBehaviour PreCreate(Type type, ulong entity, int slot)
        {
            var script = Construct(type, entity, slot);
            preCreated[(entity, slot)] = script;
            return script;
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
                    if (p == typeof(Collision2D))
                    {
                        var a = (Action<Collision2D>)Delegate.CreateDelegate(typeof(Action<Collision2D>), script, m);
                        return other => a(new Collision2D(other));
                    }
                    if (p == typeof(Collider2D))
                    {
                        var a = (Action<Collider2D>)Delegate.CreateDelegate(typeof(Action<Collider2D>), script, m);
                        return other => a(new Collider2D(other));
                    }
                    if (p == typeof(GameObject))
                    {
                        var a = (Action<GameObject>)Delegate.CreateDelegate(typeof(Action<GameObject>), script, m);
                        return other => a(new GameObject(other));
                    }
                    if (p == typeof(ControllerColliderHit))
                    {
                        var a = (Action<ControllerColliderHit>)Delegate.CreateDelegate(typeof(Action<ControllerColliderHit>), script, m);
                        return other => a(new ControllerColliderHit(other));
                    }
                    // OnAnimatorIK(int layerIndex): the layer index travels as `other`.
                    if (p == typeof(int) && name == "OnAnimatorIK")
                    {
                        var a = (Action<int>)Delegate.CreateDelegate(typeof(Action<int>), script, m);
                        return layer => a((int)layer);
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

        /// <summary>A call argument from the engine: kind -1 none, 0 bool, 1 number, 2 whole
        /// number, 3 text, 40 an object the engine hands over (csharp_input.rs).</summary>
        static object Arg(int kind, double num, char* text, int len) => kind switch
        {
            0 => num != 0,
            1 => num,
            2 => (long)num,
            3 => new string(text, 0, len),
            40 => InputBridge.TakeArg(),
            _ => null,
        };

        /// <summary>Call method `name` of script `handle` by name (SendMessage, the Inspector's
        /// On Click calls): one with no parameters, or one parameter the argument converts to.
        /// Returns 1 when the class has such a method.</summary>
        [UnmanagedCallersOnly]
        public static int Message(int handle, char* name, int len, int kind, double num, char* text, int textLen)
        {
            var e = Lookup(handle);
            if (e == null) return 0;
            if (pendingRefs.Count > 0) ResolvePending();
            string method = new string(name, 0, len);
            object arg = Arg(kind, num, text, textLen);
            return CallByName(e.script, e.typeName, method, arg) ? 1 : 0;
        }

        /// <summary>Call method `method` of `script` by name: one with no parameters, or one
        /// parameter `arg` converts to. Whether the class has such a method.</summary>
        static bool CallByName(MonoBehaviour script, string typeName, string method, object arg)
        {
            const BindingFlags flags = BindingFlags.Instance | BindingFlags.Public | BindingFlags.NonPublic;
            MethodInfo best = null;
            object[] args = null;
            for (var t = script.GetType(); t != null && t != typeof(MonoBehaviour) && best == null; t = t.BaseType)
            {
                foreach (var m in t.GetMethods(flags | BindingFlags.DeclaredOnly))
                {
                    if (m.Name != method || m.IsGenericMethodDefinition) continue;
                    var ps = m.GetParameters();
                    if (ps.Length == 1 && arg != null && TryConvert(arg, ps[0].ParameterType, out var converted))
                    {
                        best = m;
                        args = new[] { converted };
                        break;
                    }
                    if (ps.Length == 0 && best == null)
                    {
                        best = m;
                        args = Array.Empty<object>();
                    }
                }
            }
            if (best == null) return false;
            try
            {
                var result = best.Invoke(script, args);
                if (result is System.Collections.IEnumerator routine) script.StartCoroutine(routine);
            }
            catch (Exception ex)
            {
                Report(typeName, method, ex);
            }
            return true;
        }

        /// <summary>`SendMessage` (`dir` 0), `BroadcastMessage` (1: the object and everything
        /// under it) and `SendMessageUpwards` (2: the object and its parents). C# scripts are
        /// called now; Lua scripts on the same objects right after the calling callback.</summary>
        internal static void SendMessage(ulong entity, string method, object value, int dir, SendMessageOptions options)
        {
            if (string.IsNullOrEmpty(method)) return;
            var targets = new List<Transform>();
            var start = new Transform(entity);
            if (dir == 1)
            {
                var stack = new Stack<Transform>();
                stack.Push(start);
                while (stack.Count > 0)
                {
                    var t = stack.Pop();
                    targets.Add(t);
                    for (int i = t.childCount - 1; i >= 0; i--) stack.Push(t.GetChild(i));
                }
            }
            else
            {
                for (var t = start; t != null; t = dir == 2 ? t.parent : null) targets.Add(t);
            }
            bool found = false;
            foreach (var t in targets)
            {
                ulong id = t.EntityId;
                if (!Native.Flag(id, 1)) continue;
                if (byObject.TryGetValue(id, out var list))
                {
                    foreach (var script in list.ToArray())
                    {
                        if (!script.Alive) continue;
                        if (CallByName(script, script.GetType().Name, method, value)) found = true;
                    }
                }
                if (Native.QueueMessage(id, method, value)) found = true;
            }
            if (!found && options == SendMessageOptions.RequireReceiver)
                Native.Log(2, $"SendMessage {method} has no receiver!");
        }

        static bool TryConvert(object value, Type t, out object converted)
        {
            converted = null;
            try
            {
                if (t.IsInstanceOfType(value)) converted = value;
                else if (t == typeof(string)) converted = Convert.ToString(value, System.Globalization.CultureInfo.InvariantCulture);
                else if (value is string) return false;
                else if (t == typeof(bool)) converted = value is bool b ? b : Convert.ToDouble(value) != 0;
                else if (t == typeof(float)) converted = Convert.ToSingle(value);
                else if (t == typeof(double)) converted = Convert.ToDouble(value);
                else if (t == typeof(int)) converted = (int)Math.Round(Convert.ToDouble(value));
                else if (t == typeof(long)) converted = (long)Math.Round(Convert.ToDouble(value));
                else if (t == typeof(object) || t.IsInstanceOfType(value)) converted = value;
                else return false;
                return true;
            }
            catch (Exception) { return false; }
        }

        /// <summary>Run the C# listeners of UI event `kind` on `entity`; 1 when there were any.</summary>
        [UnmanagedCallersOnly]
        public static int UiEvent(ulong entity, int kind, int argKind, double num, char* text, int len)
        {
            if (pendingRefs.Count > 0) ResolvePending();
            if (kind >= 100) return InputEvents.Run(entity, kind, Arg(argKind, num, text, len)) ? 1 : 0;
            return UiEvents.Run(entity, kind, Arg(argKind, num, text, len)) ? 1 : 0;
        }

        /// <summary>Advance every running coroutine (once per frame, after Update).</summary>
        [UnmanagedCallersOnly]
        public static void Tick() => Coroutines.Tick();

        /// <summary>Script `removed` of `entity` left its Scripts list (Destroy(script)): the
        /// later scripts move down one slot.</summary>
        [UnmanagedCallersOnly]
        public static void ShiftSlots(ulong entity, int removed)
        {
            if (!byObject.TryGetValue(entity, out var list)) return;
            foreach (var other in list)
                if (other.slot > removed) other.slot--;
        }

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
                // By reference: scripts on one object compare equal (Object.Equals).
                list.RemoveAll(x => ReferenceEquals(x, s));
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
            Time.scale = timeScale;
            Time.frameCount = frame;
            for (int i = timed.Count - 1; i >= 0; i--)
            {
                if (timed[i].at <= time)
                {
                    Native.Destroy(timed[i].id);
                    timed.RemoveAt(i);
                }
            }
            AsyncOperation.Tick();
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
                // Removed at the end of the frame (OnDisable, OnDestroy).
                Native.ComponentOp(mb.entity, 4, mb.slot.ToString(System.Globalization.CultureInfo.InvariantCulture));
                return;
            }
            // An engine component: remove it (not the object), now.
            if (obj is Component c && !(obj is Transform))
            {
                string name = c is MeshRenderer ? "MeshRenderer" : c.GetType().Name;
                if (c is Collider || c is Collider2D)
                {
                    Native.Log(1, $"Destroy({name}): name the collider type to remove it (GetComponent<BoxCollider>()); nothing was removed");
                    return;
                }
                Native.ComponentOp(obj.ObjectId, 1, name);
                return;
            }
            if (t > 0f) timed.Add((obj.ObjectId, Time.time + t));
            else Native.Destroy(obj.ObjectId);
        }

        /// <summary>The engine component of type `t` on `entity` (Rigidbody, Collider,
        /// AudioSource, Animator, Camera, Light, Renderer), or null.</summary>
        internal static Component EngineComponentOf(Type t, ulong entity) => EngineComponent(t, entity);

        static Component EngineComponent(Type t, ulong entity)
        {
            if (InputBridge.Component(t, entity) is Component input) return input;
            int kind = t == typeof(Rigidbody) ? 0 : t == typeof(Collider) ? 1
                : t == typeof(AudioSource) ? 2 : t == typeof(Animator) ? 3
                : t == typeof(Camera) ? 4 : t == typeof(Light) ? 5
                : t == typeof(Renderer) || t == typeof(MeshRenderer) ? 6
                : t == typeof(Text) ? 10 : t == typeof(Image) ? 11 : t == typeof(Button) ? 12
                : t == typeof(Toggle) ? 13 : t == typeof(Slider) ? 14 : t == typeof(InputField) ? 15
                : t == typeof(Rigidbody2D) ? 20 : t == typeof(Collider2D) ? 21
                : t == typeof(NavMeshAgent) ? 22 : t == typeof(ParticleSystem) ? 23
                : t == typeof(BoxCollider) ? 30 : t == typeof(SphereCollider) ? 31
                : t == typeof(CapsuleCollider) ? 32 : t == typeof(MeshCollider) ? 33
                : t == typeof(BoxCollider2D) ? 34 : t == typeof(CircleCollider2D) ? 35 : t == typeof(CharacterController) ? 7 : -1;
            if (kind < 0 || Native.api.hasComponent(entity, kind) == 0) return null;
            return kind switch
            {
                0 => new Rigidbody(entity),
                1 => new Collider(entity),
                2 => new AudioSource(entity),
                3 => new Animator(entity),
                4 => new Camera(entity),
                5 => new Light(entity),
                6 => new MeshRenderer(entity),
                10 => new Text(entity),
                11 => new Image(entity),
                12 => new Button(entity),
                13 => new Toggle(entity),
                14 => new Slider(entity),
                15 => new InputField(entity),
                20 => new Rigidbody2D(entity),
                21 => new Collider2D(entity),
                22 => new NavMeshAgent(entity),
                23 => new ParticleSystem(entity),
                30 => new BoxCollider(entity),
                31 => new SphereCollider(entity),
                32 => new CapsuleCollider(entity),
                33 => new MeshCollider(entity),
                34 => new BoxCollider2D(entity),
                35 => new CircleCollider2D(entity),
                _ => new CharacterController(entity),
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

// ---- 2D / 3D contact points and the Animator IK pass (csharp_parity.rs) ----------------------------

namespace Misty
{
    /// <summary>One touching point of a 2D collision (`Collision2D.GetContact`).</summary>
    public struct ContactPoint2D
    {
        internal ulong colliderId, otherColliderId;
        /// <summary>World position of the point.</summary>
        public Vector2 point { get; internal set; }
        /// <summary>Surface normal, pointing away from the other collider (towards this one).</summary>
        public Vector2 normal { get; internal set; }
        /// <summary>Distance between the surfaces along the normal (negative: overlapping).</summary>
        public float separation { get; internal set; }
        /// <summary>Impulse the solver applied along the normal at this point in the step.</summary>
        public float normalImpulse { get; internal set; }
        /// <summary>Friction impulse the solver applied along the surface in the step.</summary>
        public float tangentImpulse { get; internal set; }
        /// <summary>Velocity of the other object relative to this one.</summary>
        public Vector2 relativeVelocity { get; internal set; }
        public bool enabled => colliderId != 0;
        /// <summary>The other object's collider.</summary>
        public Collider2D collider => colliderId == 0 ? null : new Collider2D(colliderId);
        /// <summary>This object's collider.</summary>
        public Collider2D otherCollider => otherColliderId == 0 ? null : new Collider2D(otherColliderId);
        public Rigidbody2D rigidbody => collider?.attachedRigidbody;
        public Rigidbody2D otherRigidbody => otherCollider?.attachedRigidbody;
    }

    public sealed unsafe partial class Collision2D
    {
        ContactPoint2D[] points = Array.Empty<ContactPoint2D>();
        ulong me;

        /// <summary>Copies the contact points of the step that fired the callback.</summary>
        void Capture()
        {
            const int Floats = 7, Max = 8;
            float* buf = stackalloc float[2 + Floats * Max];
            ulong* ids = stackalloc ulong[2];
            int n = Internal.Native.api.contacts(0, buf, 2 + Floats * Max, ids);
            if (n < 0) return;
            n = Math.Min(n, Max);
            me = ids[0];
            relativeVelocity = new Vector2(buf[0], buf[1]);
            points = new ContactPoint2D[n];
            for (int i = 0; i < n; i++)
            {
                float* p = buf + 2 + i * Floats;
                points[i] = new ContactPoint2D
                {
                    colliderId = id,
                    otherColliderId = me,
                    point = new Vector2(p[0], p[1]),
                    normal = new Vector2(p[2], p[3]),
                    separation = p[4],
                    normalImpulse = p[5],
                    tangentImpulse = p[6],
                    relativeVelocity = relativeVelocity,
                };
            }
        }

        /// <summary>Velocity of the other object relative to this one (before the step for Enter).</summary>
        public Vector2 relativeVelocity { get; private set; }
        /// <summary>This object's collider.</summary>
        public Collider2D otherCollider => me == 0 ? null : new Collider2D(me);
        public Rigidbody2D otherRigidbody => otherCollider?.attachedRigidbody;
        public bool enabled => true;
        public int contactCount => points.Length;
        /// <summary>A copy of the contact points (allocates; prefer GetContact / GetContacts).</summary>
        public ContactPoint2D[] contacts => (ContactPoint2D[])points.Clone();
        public ContactPoint2D GetContact(int index)
        {
            if ((uint)index >= (uint)points.Length)
                throw new IndexOutOfRangeException($"Contact index {index} is out of range (contactCount is {points.Length}).");
            return points[index];
        }
        /// <summary>Fills `contacts` (as many as fit); returns how many were written.</summary>
        public int GetContacts(ContactPoint2D[] contacts)
        {
            int n = Math.Min(contacts.Length, points.Length);
            Array.Copy(points, contacts, n);
            return n;
        }
        /// <summary>Replaces the list's contents with the contact points; returns the count.</summary>
        public int GetContacts(List<ContactPoint2D> contacts)
        {
            contacts.Clear();
            contacts.AddRange(points);
            return points.Length;
        }
    }

    /// <summary>What a Character Controller ran into while moving (`OnControllerColliderHit`).</summary>
    public sealed unsafe class ControllerColliderHit
    {
        readonly ulong other, self;
        internal ControllerColliderHit(ulong other)
        {
            this.other = other;
            float* buf = stackalloc float[10];
            ulong* ids = stackalloc ulong[2];
            if (Internal.Native.api.contacts(2, buf, 10, ids) < 0) return;
            self = ids[0];
            point = new Vector3(buf[0], buf[1], buf[2]);
            normal = new Vector3(buf[3], buf[4], buf[5]);
            moveDirection = new Vector3(buf[6], buf[7], buf[8]);
            moveLength = buf[9];
        }
        /// <summary>The controller that hit.</summary>
        public CharacterController controller => self == 0 ? null : new CharacterController(self);
        public GameObject gameObject => new GameObject(other);
        public Transform transform => new Transform(other);
        public Collider collider => new Collider(other);
        public Rigidbody rigidbody => gameObject.GetComponentInParent<Rigidbody>();
        /// <summary>World position of the touch.</summary>
        public Vector3 point { get; }
        /// <summary>Surface normal at the touch.</summary>
        public Vector3 normal { get; }
        /// <summary>Direction the controller was moving.</summary>
        public Vector3 moveDirection { get; }
        /// <summary>How far the controller moved before the hit.</summary>
        public float moveLength { get; }
    }

    /// <summary>One touching point of a 3D collision (`Collision.GetContact`).</summary>
    public struct ContactPoint
    {
        internal ulong thisId, otherId;
        /// <summary>World position of the point.</summary>
        public Vector3 point { get; internal set; }
        /// <summary>Surface normal, pointing away from the other collider (towards this one).</summary>
        public Vector3 normal { get; internal set; }
        /// <summary>Distance between the surfaces along the normal (negative: overlapping).</summary>
        public float separation { get; internal set; }
        /// <summary>Impulse the solver applied at this point in the step (along the normal).</summary>
        public Vector3 impulse { get; internal set; }
        /// <summary>The collider of the object whose callback runs.</summary>
        public Collider thisCollider => thisId == 0 ? null : new Collider(thisId);
        /// <summary>The other object's collider.</summary>
        public Collider otherCollider => otherId == 0 ? null : new Collider(otherId);
    }

    public sealed unsafe partial class Collision
    {
        ContactPoint[] points = Array.Empty<ContactPoint>();

        /// <summary>Copies the contact points of the step that fired the callback.</summary>
        void Capture()
        {
            const int Floats = 8, Max = 16;
            float* buf = stackalloc float[6 + Floats * Max];
            ulong* ids = stackalloc ulong[2];
            int n = Internal.Native.api.contacts(1, buf, 6 + Floats * Max, ids);
            if (n < 0) return;
            n = Math.Min(n, Max);
            ulong me = ids[0];
            relativeVelocity = new Vector3(buf[0], buf[1], buf[2]);
            impulse = new Vector3(buf[3], buf[4], buf[5]);
            points = new ContactPoint[n];
            for (int i = 0; i < n; i++)
            {
                float* p = buf + 6 + i * Floats;
                var normal = new Vector3(p[3], p[4], p[5]);
                points[i] = new ContactPoint
                {
                    thisId = me,
                    otherId = id,
                    point = new Vector3(p[0], p[1], p[2]),
                    normal = normal,
                    separation = p[6],
                    impulse = normal * p[7],
                };
            }
        }

        /// <summary>Velocity of the other object relative to this one (before the step, so an impact reports the speed it hit with).</summary>
        public Vector3 relativeVelocity { get; private set; }
        /// <summary>Total impulse applied to this object by the contact in the step.</summary>
        public Vector3 impulse { get; private set; }
        public int contactCount => points.Length;
        /// <summary>A copy of the contact points (allocates; prefer GetContact / GetContacts).</summary>
        public ContactPoint[] contacts => (ContactPoint[])points.Clone();
        public ContactPoint GetContact(int index)
        {
            if ((uint)index >= (uint)points.Length)
                throw new IndexOutOfRangeException($"Contact index {index} is out of range (contactCount is {points.Length}).");
            return points[index];
        }
        /// <summary>Fills `contacts` (as many as fit); returns how many were written.</summary>
        public int GetContacts(ContactPoint[] contacts)
        {
            int n = Math.Min(contacts.Length, points.Length);
            Array.Copy(points, contacts, n);
            return n;
        }
        /// <summary>Replaces the list's contents with the contact points; returns the count.</summary>
        public int GetContacts(List<ContactPoint> contacts)
        {
            contacts.Clear();
            contacts.AddRange(points);
            return points.Length;
        }
    }

    public enum AvatarIKGoal { LeftFoot = 0, RightFoot = 1, LeftHand = 2, RightHand = 3 }
    public enum AvatarIKHint { LeftKnee = 0, RightKnee = 1, LeftElbow = 2, RightElbow = 3 }

    /// <summary>Human bones, numbered like the familiar engine (bones past RightToes are not mapped).</summary>
    public enum HumanBodyBones
    {
        Hips = 0, LeftUpperLeg = 1, RightUpperLeg = 2, LeftLowerLeg = 3, RightLowerLeg = 4,
        LeftFoot = 5, RightFoot = 6, Spine = 7, Chest = 8, Neck = 9, Head = 10,
        LeftShoulder = 11, RightShoulder = 12, LeftUpperArm = 13, RightUpperArm = 14,
        LeftLowerArm = 15, RightLowerArm = 16, LeftHand = 17, RightHand = 18,
        LeftToes = 19, RightToes = 20, LeftEye = 21, RightEye = 22, Jaw = 23,
        UpperChest = 54, LastBone = 55,
    }

    /// <summary>The IK pass: call these from `OnAnimatorIK(int layerIndex)` (a controller layer
    /// needs IK Pass on). Goals reset every frame.</summary>
    public sealed unsafe partial class Animator
    {
        /// <summary>Four floats for `animatorIk`: a position, rotation or weights.</summary>
        struct F4 { public float x, y, z, w; public F4(float x, float y = 0, float z = 0, float w = 0) { this.x = x; this.y = y; this.z = z; this.w = w; } }

        F4 Ik(int op, int index, F4 v = default)
        {
            if (Internal.Native.api.animatorIk(id, op, index, (float*)&v) == 0)
            {
                if (!Alive) throw Internal.Native.Missing("Animator");
                throw new ArgumentOutOfRangeException(nameof(index));
            }
            return v;
        }

        static F4 V(Vector3 p) => new F4(p.x, p.y, p.z);
        static F4 V(Quaternion q) => new F4(q.x, q.y, q.z, q.w);
        static F4 V(float f) => new F4(f);
        static Vector3 P(F4 v) => new Vector3(v.x, v.y, v.z);
        static Quaternion Q(F4 v) => new Quaternion(v.x, v.y, v.z, v.w);

        public void SetIKPosition(AvatarIKGoal goal, Vector3 goalPosition) => Ik(0, (int)goal, V(goalPosition));
        /// <summary>Humanoid effector rotation: identity is the bone as in the Avatar's rest pose
        /// with the character unrotated.</summary>
        public void SetIKRotation(AvatarIKGoal goal, Quaternion goalRotation) => Ik(1, (int)goal, V(goalRotation));
        public void SetIKPositionWeight(AvatarIKGoal goal, float value) => Ik(2, (int)goal, V(value));
        public void SetIKRotationWeight(AvatarIKGoal goal, float value) => Ik(3, (int)goal, V(value));
        /// <summary>The goal set this frame, or where the animation put the hand / foot.</summary>
        public Vector3 GetIKPosition(AvatarIKGoal goal) => P(Ik(4, (int)goal));
        public Quaternion GetIKRotation(AvatarIKGoal goal) => Q(Ik(5, (int)goal));
        public float GetIKPositionWeight(AvatarIKGoal goal) => Ik(6, (int)goal).x;
        public float GetIKRotationWeight(AvatarIKGoal goal) => Ik(7, (int)goal).x;
        public void SetIKHintPosition(AvatarIKHint hint, Vector3 hintPosition) => Ik(8, (int)hint, V(hintPosition));
        public void SetIKHintPositionWeight(AvatarIKHint hint, float value) => Ik(9, (int)hint, V(value));
        public Vector3 GetIKHintPosition(AvatarIKHint hint) => P(Ik(10, (int)hint));
        public float GetIKHintPositionWeight(AvatarIKHint hint) => Ik(11, (int)hint).x;
        public void SetLookAtPosition(Vector3 lookAtPosition) => Ik(12, 0, V(lookAtPosition));
        public void SetLookAtWeight(float weight, float bodyWeight = 0f, float headWeight = 1f, float eyesWeight = 0f, float clampWeight = 0.5f)
        {
            Ik(13, 0, new F4(weight, bodyWeight, headWeight, eyesWeight));
            Ik(14, 0, V(clampWeight));
        }
        /// <summary>The Transform of a human bone (from the Avatar, or found by name), or null.</summary>
        public Transform GetBoneTransform(HumanBodyBones humanBoneId)
        {
            var v = new F4();
            if (Internal.Native.api.animatorIk(id, 15, (int)humanBoneId, (float*)&v) == 0)
            {
                if (!Alive) throw Internal.Native.Missing("Animator");
                return null;
            }
            ulong lo = (uint)BitConverter.SingleToInt32Bits(v.x);
            ulong hi = (uint)BitConverter.SingleToInt32Bits(v.y);
            ulong bone = lo | (hi << 32);
            return bone == 0 ? null : new Transform(bone);
        }
    }
}

namespace Misty
{
    // Input System bindings and interactive rebinding (`csharp_rebinding.rs`).

    /// <summary>A binding of an action (`action.bindings[i]`).</summary>
    public struct InputBinding
    {
        /// <summary>The composite's name ("WASD") or the part's ("up"); "" for a plain binding.</summary>
        public string name { get; internal set; }
        /// <summary>The authored path (`&lt;Keyboard&gt;/space`; "" for a composite).</summary>
        public string path { get; internal set; }
        /// <summary>The override (rebind), or null when there is none.</summary>
        public string overridePath { get; internal set; }
        public string effectivePath => overridePath ?? path;
        /// <summary>Control scheme groups, `;`-separated ("Keyboard&amp;Mouse").</summary>
        public string groups { get; internal set; }
        /// <summary>The stable id the saved overrides refer to.</summary>
        public string id { get; internal set; }
        public bool isComposite { get; internal set; }
        public bool isPartOfComposite { get; internal set; }
        public bool hasOverrides => overridePath != null;
        /// <summary>The action's name.</summary>
        public string action { get; internal set; }

        internal bool Matches(string group, string p)
        {
            if (!string.IsNullOrEmpty(group))
            {
                bool found = false;
                foreach (var g in (groups ?? "").Split(';'))
                    if (g.Trim() == group) found = true;
                if (!found) return false;
            }
            return string.IsNullOrEmpty(p)
                || string.Equals(effectivePath, p, StringComparison.OrdinalIgnoreCase)
                || string.Equals(path, p, StringComparison.OrdinalIgnoreCase);
        }

        internal static unsafe InputBinding[] Of(InputAction a)
        {
            long packed = ((long)a.map << 32) | (long)(uint)a.action;
            int n = (int)InputBridge.Check(InputBridge.Call(85, (ulong)a.set, packed));
            var list = new InputBinding[n];
            string actionName = a.name;
            double* v = stackalloc double[16];
            char* buf = stackalloc char[2048];
            for (int i = 0; i < n; i++)
            {
                v[0] = i;
                long bits = InputBridge.Check(Internal.Native.api.inputSystem(86, (ulong)a.set, packed, v, buf, 2048));
                var parts = new string(buf, 0, (int)Math.Clamp(v[15], 0, 2048)).Split('\n');
                list[i] = new InputBinding
                {
                    name = parts.Length > 0 ? parts[0] : "",
                    path = parts.Length > 1 ? parts[1] : "",
                    overridePath = (bits & 4) != 0 && parts.Length > 2 ? parts[2] : null,
                    groups = parts.Length > 3 ? parts[3] : "",
                    id = parts.Length > 4 ? parts[4] : "",
                    isComposite = (bits & 1) != 0,
                    isPartOfComposite = (bits & 2) != 0,
                    action = actionName,
                };
            }
            return list;
        }

        public override string ToString() =>
            isComposite ? $"{name}:{action}" : $"{effectivePath}{(string.IsNullOrEmpty(groups) ? "" : $"[{groups}]")}";
    }

    /// <summary>Interactive rebinding (`action.PerformInteractiveRebinding()`).</summary>
    public static class InputActionRebindingExtensions
    {
        /// <summary>A rebinding in progress: configure it (`WithControlsExcluding`,
        /// `WithCancelingThrough`, `OnComplete`, …), then `Start()` it. It listens for the next
        /// key, mouse button, gamepad button, trigger or stick direction (a whole stick for a
        /// stick binding) and binds it as an override; Escape cancels unless changed.</summary>
        public sealed unsafe class RebindingOperation : IDisposable
        {
            ulong id;
            int lastBinding = -1;
            Action<RebindingOperation> onComplete, onCancel;
            // Action<string>, not Action<object>: InputEvents.Run would take an Action<object>
            // for an Action<PlayerInput> (delegate variance).
            readonly Action<string> completeListener, cancelListener;

            RebindingOperation(InputAction action, ulong id)
            {
                this.action = action;
                this.id = id;
                RememberBinding();
                completeListener = path => Finish(path ?? "");
                cancelListener = _ => Finish(null);
                InputEvents.Add(Key, 140, completeListener);
                InputEvents.Add(Key, 141, cancelListener);
            }

            ulong Key => (1UL << 62) | id;

            internal static RebindingOperation Create(InputAction action, int bindingIndex)
            {
                double* v = stackalloc double[16];
                v[0] = bindingIndex;
                long packed = ((long)action.map << 32) | (long)(uint)action.action;
                long id = Internal.Native.api.inputSystem(130, (ulong)action.set, packed, v, null, 0);
                if (id < 0) throw new InvalidOperationException(InputBridge.LastError());
                return new RebindingOperation(action, (ulong)id);
            }

            public InputAction action { get; }
            /// <summary>Listening (or finished after listening).</summary>
            public bool started { get; private set; }
            public bool completed { get; private set; }
            public bool canceled { get; private set; }
            /// <summary>The control that was picked (null until completed).</summary>
            public InputControl selectedControl { get; private set; }
            /// <summary>The binding being rebound.</summary>
            public int bindingIndex
            {
                get
                {
                    RememberBinding();
                    return lastBinding;
                }
            }

            void Op(int op, string text = null, double value = 0)
            {
                if (id == 0) throw new InvalidOperationException("The rebinding operation is over (completed, canceled or disposed)");
                double* v = stackalloc double[16];
                v[0] = value;
                InputBridge.Check(InputBridge.WithText(op, id, 0, text ?? "", v));
            }

            /// <summary>Never pick controls whose path contains `path` ("&lt;Mouse&gt;", "Mouse",
            /// "&lt;Keyboard&gt;/escape").</summary>
            public RebindingOperation WithControlsExcluding(string path) { Op(131, path); return this; }
            /// <summary>Pressing this control cancels (default `&lt;Keyboard&gt;/escape`).</summary>
            public RebindingOperation WithCancelingThrough(string path) { Op(132, path); return this; }
            public RebindingOperation WithTargetBinding(int bindingIndex) { Op(133, null, bindingIndex); RememberBinding(); return this; }
            /// <summary>Rebind the binding of control scheme group `group` ("Gamepad") and only
            /// listen to that scheme's devices.</summary>
            public RebindingOperation WithBindingGroup(string group) { Op(134, group); RememberBinding(); return this; }
            /// <summary>After a match, keep listening `seconds` for another control (the last one wins).</summary>
            public RebindingOperation OnMatchWaitForAnother(float seconds) { Op(135, null, seconds); return this; }
            public RebindingOperation OnComplete(Action<RebindingOperation> callback) { onComplete = callback; return this; }
            public RebindingOperation OnCancel(Action<RebindingOperation> callback) { onCancel = callback; return this; }

            /// <summary>Start listening (from the next frame). The action is disabled until the
            /// operation ends, and comes back once the picked control is released.</summary>
            public RebindingOperation Start()
            {
                if (started) return this;
                Op(136);
                started = true;
                return this;
            }

            /// <summary>Stop listening; `OnCancel` runs when it was started.</summary>
            public void Cancel()
            {
                if (id == 0) return;
                RememberBinding();
                bool wasListening = InputBridge.Call(137, id) == 1;
                if (wasListening) Finish(null);
                else Release();
            }

            /// <summary>Stop and forget the operation (no callback).</summary>
            public void Dispose()
            {
                if (id == 0) return;
                RememberBinding();
                InputBridge.Call(138, id);
                Release();
            }

            void RememberBinding()
            {
                if (id == 0) return;
                double* v = stackalloc double[16];
                if (Internal.Native.api.inputSystem(139, id, 0, v, null, 0) == 1) lastBinding = (int)v[0];
            }

            void Release()
            {
                if (id == 0) return;
                InputEvents.Remove(Key, 140, completeListener);
                InputEvents.Remove(Key, 141, cancelListener);
                id = 0;
            }

            void Finish(string path)
            {
                if (completed || canceled) return;
                Release();
                if (path != null)
                {
                    completed = true;
                    selectedControl = new InputControl(path);
                    onComplete?.Invoke(this);
                }
                else
                {
                    canceled = true;
                    onCancel?.Invoke(this);
                }
            }

            public override string ToString() => $"Rebind {action} #{lastBinding}";
        }
    }
}
