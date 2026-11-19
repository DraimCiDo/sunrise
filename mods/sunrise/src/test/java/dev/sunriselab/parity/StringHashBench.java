package dev.sunriselab.parity;

/**
 * HotSpot's String.hashCode per call on fresh strings, for tools/string_hash_kit: MIX LEN ROUNDS [stepped|check].
 * Each batch makes the row's strings anew (untimed: new String(char[], off, len), compact Latin-1 when every unit
 * fits), then times one hashCode call on each, so that no hash is cached. The strings are the same as
 * tools/string_hash_kit/bench.rs makes.
 */
public final class StringHashBench {
    static volatile int sink;
    static String[] fresh;

    public static void main(String[] a) throws Exception {
        String mix = a[0];
        int len = Integer.parseInt(a[1]);
        int count = Math.max(64, Math.min(4096, 32768 / Math.max(len, 1)));
        char[] pool = new char[count * len];
        long state = 1;
        for (int i = 0; i < count; i++) {
            int n = 0;
            while (n < len) {
                state = state * 6364136223846793005L + 1442695040888963407L;
                int r = (int) (state >>> 33);
                int at = i * len + n;
                switch (mix) {
                    case "ascii" -> { pool[at] = (char) (0x20 + r % 95); n++; }
                    case "latin1" -> { pool[at] = (char) (0x20 + r % 224); n++; }
                    case "bmp" -> { pool[at] = (char) (0x100 + r % (0xD800 - 0x100)); n++; }
                    case "surrogates" -> {
                        if (n + 1 < len) {
                            pool[at] = (char) (0xD800 + r % 0x400);
                            pool[at + 1] = (char) (0xDC00 + (r >>> 10) % 0x400);
                            n += 2;
                        } else { pool[at] = (char) (0x100 + r % (0xD800 - 0x100)); n++; }
                    }
                    default -> throw new IllegalArgumentException("unknown mix " + mix);
                }
            }
        }
        fresh = new String[count];
        if (a.length > 3 && a[3].equals("check")) {
            make(pool, len);
            System.out.println(hashAll());
            return;
        }
        int rounds = Integer.parseInt(a[2]);
        boolean stepped = a.length > 3 && a[3].equals("stepped");
        java.io.BufferedReader in = new java.io.BufferedReader(new java.io.InputStreamReader(System.in));
        for (int round = 0; round < rounds; round++) {
            if (stepped && in.readLine() == null) return;
            for (int w = 0; w < 2000; w++) { make(pool, len); sink = hashAll(); }
            double[] t = new double[400];
            for (int b = 0; b < 400; b++) {
                make(pool, len);
                long s = System.nanoTime();
                sink = hashAll();
                t[b] = (System.nanoTime() - s) / (double) count;
            }
            java.util.Arrays.sort(t);
            System.out.printf("hotspot median %.4f ns%n", t[200]);
            System.out.flush();
        }
    }

    static void make(char[] pool, int len) {
        for (int i = 0; i < fresh.length; i++) fresh[i] = new String(pool, i * len, len);
    }

    static int hashAll() {
        int s = 0;
        for (String x : fresh) s += x.hashCode();
        return s;
    }
}
