// Makes tools/golden/natural.txt: the Natural Temperature mod's own PainterCore (calculateTemperature / calculateHumidity) on random
// settings, one line a case (the settings, the seed, the point, the raw noise, the two results).
//   * the mod's classes (ModConfig, MathUtils, Painter/PainterCore, WorldSeedHolder, Temperature/*, Humidity/*, GenMode), their class
//     file version patched from 69 to 65 so Java 21 loads them;
//   * stubs instead of Minecraft and Fabric: org.slf4j.Logger, DensitySampler.Bound (abstract class, sampleValue(III)F),
//     NoiseBiomeBandsGenerator (getNoise = sin(x*s*0.00013 + z*s*0.00007)*3 + cos(z*s*0.00011), getDetailNoise = sin(x*s*0.011) *
//     cos(z*s*0.017) * 0.8), and NaturalTemperature (CONFIG plus the 16 registry entries, as the mod's onInitialize does).
//   java -cp classes Harness 6000 > golden.txt; the checked-in file keeps 12 cases of each mode.
import com.naturaltemperature.*;
import com.naturaltemperature.Painter.PainterCore;
import net.minecraft.world.level.levelgen.densityfunction.DensitySampler;
import java.util.*;
import java.lang.reflect.*;

public class Harness {
    static Random R = new Random(20260611L);
    static double u(double lo, double hi) { return lo + R.nextDouble() * (hi - lo); }
    static double zone() { return R.nextInt(100) < 15 ? 8.0 : Math.round(u(-1, 1) * 100) / 100.0; }
    static double tp() { return R.nextInt(100) < 50 ? 8.0 : Math.round(u(-1, 1) * 100) / 100.0; }
    public static void main(String[] a) throws Exception {
        int n = Integer.parseInt(a[0]);
        Field cs = Class.forName("com.naturaltemperature.WorldSeedHolder").getDeclaredField("cachedSeed");
        cs.setAccessible(true);
        StringBuilder sb = new StringBuilder();
        for (int i = 0; i < n; i++) {
            ModConfig c = new ModConfig();
            NaturalTemperature.CONFIG = c;
            int[] modes = {0,1,2,3,4,5,6,7,8,9,10,11,12,12,12,50};
            c.generation_mode = modes[R.nextInt(modes.length)];
            c.equatorial_distance = R.nextInt(40) == 0 ? 0.0 : Math.round(u(300, 30000));
            c.equator_offset = R.nextBoolean() ? 0.0 : Math.round(u(-30000, 30000));
            c.wave_magnification = R.nextInt(10) == 0 ? 0.0 : Math.round(u(0.5, 6) * 10) / 10.0;
            c.wave_frequency = Math.round(u(0.2, 3) * 10) / 10.0;
            c.wave_amplitude = Math.round(u(0.2, 3) * 10) / 10.0;
            c.looping_world = R.nextBoolean();
            c.distortionMultiplier = 1 + R.nextInt(3);
            c.global_temperature_modifier_percentage = R.nextBoolean() ? 0 : Math.round(u(-100, 100));
            c.global_temperature_mitigation_percentage = R.nextBoolean() ? 0 : Math.round(u(0, 100));
            double[] tz = new double[12], tpp = new double[5], tv = new double[12], hz = new double[12], ht = new double[5], uv = new double[12], cov = new double[12];
            for (int k = 0; k < 12; k++) { tz[k] = zone(); tv[k] = Math.round(u(0, 1) * 100) / 100.0; hz[k] = zone(); uv[k] = Math.round(u(0, 1) * 100) / 100.0; cov[k] = R.nextInt(12) == 0 ? 0.0 : Math.round(u(1, 15) * 2) / 2.0; }
            for (int k = 0; k < 5; k++) { tpp[k] = tp(); ht[k] = tp(); }
            c.temperature_zone_1 = tz[0]; c.temperature_zone_2 = tz[1]; c.temperature_zone_3 = tz[2]; c.temperature_zone_4 = tz[3]; c.temperature_zone_5 = tz[4]; c.temperature_zone_6 = tz[5]; c.temperature_zone_7 = tz[6]; c.temperature_zone_8 = tz[7]; c.temperature_zone_9 = tz[8]; c.temperature_zone_10 = tz[9]; c.temperature_zone_11 = tz[10]; c.temperature_zone_12 = tz[11];
            c.temperature_transition_point_1 = tpp[0]; c.temperature_transition_point_2 = tpp[1]; c.temperature_transition_point_3 = tpp[2]; c.temperature_transition_point_4 = tpp[3]; c.temperature_transition_point_5 = tpp[4];
            c.t_variability_zone_1 = tv[0]; c.t_variability_zone_2 = tv[1]; c.t_variability_zone_3 = tv[2]; c.t_variability_zone_4 = tv[3]; c.t_variability_zone_5 = tv[4]; c.t_variability_zone_6 = tv[5]; c.t_variability_zone_7 = tv[6]; c.t_variability_zone_8 = tv[7]; c.t_variability_zone_9 = tv[8]; c.t_variability_zone_10 = tv[9]; c.t_variability_zone_11 = tv[10]; c.t_variability_zone_12 = tv[11];
            c.enable_humidity_bands = R.nextBoolean();
            c.humidity_bands_width = Math.round(u(200, 5000));
            c.humidity_zone_1 = hz[0]; c.humidity_zone_2 = hz[1]; c.humidity_zone_3 = hz[2]; c.humidity_zone_4 = hz[3]; c.humidity_zone_5 = hz[4]; c.humidity_zone_6 = hz[5]; c.humidity_zone_7 = hz[6]; c.humidity_zone_8 = hz[7]; c.humidity_zone_9 = hz[8]; c.humidity_zone_10 = hz[9]; c.humidity_zone_11 = hz[10]; c.humidity_zone_12 = hz[11];
            c.humidity_transition_point_1 = ht[0]; c.humidity_transition_point_2 = ht[1]; c.humidity_transition_point_3 = ht[2]; c.humidity_transition_point_4 = ht[3]; c.humidity_transition_point_5 = ht[4];
            c.u_variability_zone_1 = uv[0]; c.u_variability_zone_2 = uv[1]; c.u_variability_zone_3 = uv[2]; c.u_variability_zone_4 = uv[3]; c.u_variability_zone_5 = uv[4]; c.u_variability_zone_6 = uv[5]; c.u_variability_zone_7 = uv[6]; c.u_variability_zone_8 = uv[7]; c.u_variability_zone_9 = uv[8]; c.u_variability_zone_10 = uv[9]; c.u_variability_zone_11 = uv[10]; c.u_variability_zone_12 = uv[11];
            c.extra_bands = R.nextBoolean();
            c.percentage_coverage_zone_1 = cov[0]; c.percentage_coverage_zone_2 = cov[1]; c.percentage_coverage_zone_3 = cov[2]; c.percentage_coverage_zone_4 = cov[3]; c.percentage_coverage_zone_5 = cov[4]; c.percentage_coverage_zone_6 = cov[5]; c.percentage_coverage_zone_7 = cov[6]; c.percentage_coverage_zone_8 = cov[7]; c.percentage_coverage_zone_9 = cov[8]; c.percentage_coverage_zone_10 = cov[9]; c.percentage_coverage_zone_11 = cov[10]; c.percentage_coverage_zone_12 = cov[11];
            c.humidity_offset = R.nextBoolean() ? 0.0 : Math.round(u(-3000, 3000));
            c.randomize_underground = false;
            long seed = R.nextLong();
            cs.set(null, Long.valueOf(seed));
            // positions: mostly within +-(1.5 a) of the origin so every band is hit
            double span = Math.max(1000, c.equatorial_distance * 2.5);
            int x = (int) u(-span, span), z = (int) u(-span, span), y = 64;
            final float rt = (float) u(-1, 1), ru = (float) u(-1, 1);
            DensitySampler.Bound bt = new DensitySampler.Bound() { public float sampleValue(int a1, int b1, int c1) { return rt; } };
            DensitySampler.Bound bu = new DensitySampler.Bound() { public float sampleValue(int a1, int b1, int c1) { return ru; } };
            NoiseBiomeBandsGenerator g = new NoiseBiomeBandsGenerator(seed);
            float T = PainterCore.calculateTemperature(bt, g, x, y, z);
            float H = PainterCore.calculateHumidity(bt, bu, g, x, y, z);
            sb.setLength(0);
            sb.append(c.generation_mode).append(' ').append(c.equatorial_distance).append(' ').append(c.equator_offset).append(' ').append(c.wave_magnification).append(' ').append(c.wave_frequency).append(' ').append(c.wave_amplitude).append(' ').append(c.looping_world ? 1 : 0).append(' ').append(c.distortionMultiplier).append(' ').append(c.global_temperature_modifier_percentage).append(' ').append(c.global_temperature_mitigation_percentage);
            for (double[] arr : new double[][]{tz, tpp, tv}) for (double d : arr) sb.append(' ').append(d);
            sb.append(' ').append(c.enable_humidity_bands ? 1 : 0).append(' ').append(c.humidity_bands_width);
            for (double[] arr : new double[][]{hz, ht, uv}) for (double d : arr) sb.append(' ').append(d);
            sb.append(' ').append(c.extra_bands ? 1 : 0);
            for (double d : cov) sb.append(' ').append(d);
            sb.append(' ').append(c.humidity_offset).append(' ').append(seed).append(' ').append(x).append(' ').append(y).append(' ').append(z).append(' ').append((double) rt).append(' ').append((double) ru).append(' ').append((double) T).append(' ').append((double) H);
            System.out.println(sb);
        }
    }
}
