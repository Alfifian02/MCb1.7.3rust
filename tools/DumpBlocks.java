import java.lang.reflect.*;
import java.util.*;
import net.minecraft.src.*;

public class DumpBlocks {
    static Object f(Class<?> c, Object o, String n) throws Exception {
        while (c != null) {
            try { Field fl = c.getDeclaredField(n); fl.setAccessible(true); return fl.get(o); }
            catch (NoSuchFieldException e) { c = c.getSuperclass(); }
        }
        throw new NoSuchFieldException(n);
    }
    public static void main(String[] a) throws Exception {
        Map<Object,String> mats = new IdentityHashMap<>();
        for (Field fl : Material.class.getDeclaredFields())
            if (Modifier.isStatic(fl.getModifiers()) && fl.getType() == Material.class) { fl.setAccessible(true); mats.put(fl.get(null), fl.getName()); }
        Map<Object,String> snd = new IdentityHashMap<>();
        for (Field fl : Block.class.getDeclaredFields())
            if (Modifier.isStatic(fl.getModifiers()) && StepSound.class.isAssignableFrom(fl.getType())) { fl.setAccessible(true); snd.put(fl.get(null), fl.getName()); }
        for (int i = 0; i < 256; i++) {
            Block b = Block.blocksList[i];
            if (b == null) continue;
            try {
            Class<?> c = Block.class;
            StringBuilder s = new StringBuilder();
            s.append(i).append('|').append(b.getClass().getSimpleName());
            s.append('|').append(f(c, b, "blockName"));
            s.append('|').append(b.blockIndexInTexture);
            s.append('|').append(mats.get(b.blockMaterial));
            s.append('|').append(f(c, b, "blockHardness"));
            s.append('|').append(f(c, b, "blockResistance"));
            s.append('|').append(Block.lightValue[i]);
            s.append('|').append(Block.lightOpacity[i]);
            s.append('|').append(b.isOpaqueCube());
            s.append('|').append(b.renderAsNormalBlock());
            s.append('|').append(b.getRenderType());
            s.append('|').append(b.isCollidable());
            s.append('|').append(snd.get(b.stepSound));
            s.append('|').append(b.minX).append(',').append(b.minY).append(',').append(b.minZ).append(',').append(b.maxX).append(',').append(b.maxY).append(',').append(b.maxZ);
            s.append('|').append(Block.tickOnLoad[i]);
            s.append('|').append(Block.isBlockContainer[i]);
            s.append('|').append(Block.canBlockGrass[i]);
            s.append('|').append(Block.field_28032_t[i]);
            s.append('|').append(f(c, b, "enableStats"));
            s.append('|').append(b.slipperiness);
            s.append('|').append(b.tickRate());
            System.out.println(s);
            } catch (Throwable t) { System.out.println("ERR|" + i + "|" + t); }
        }
    }
}
