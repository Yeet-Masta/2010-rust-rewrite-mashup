import java.io.*;
import java.lang.reflect.Method;
import java.util.*;
import net.minecraft.SharedConstants;
import net.minecraft.core.BlockPos;
import net.minecraft.core.registries.BuiltInRegistries;
import net.minecraft.server.Bootstrap;
import net.minecraft.world.level.EmptyBlockGetter;
import net.minecraft.world.level.block.Block;
import net.minecraft.world.level.block.state.BlockState;
import net.minecraft.world.phys.Vec3;
import net.minecraft.world.phys.shapes.VoxelShape;

// Writes minecraft/data/block-outline-26.3.json (gzip it after). Run against
// the 26.3 client JAR and its libraries (from version.json) on Java 25:
//   javac -proc:none -cp "client.jar:libs/*" DumpShapes.java
//   java -cp "client.jar:libs/*:." DumpShapes block-outline-26.3.json

/** Each block state's outline shape (getShape with an empty context, at the origin, its offset taken back off) as the edges forAllEdges walks, and each offset block's offset type and limits. */
public class DumpShapes {
    public static void main(String[] args) throws Exception {
        SharedConstants.tryDetectVersion();
        Bootstrap.bootStrap();
        Map<String, Integer> shapeIndex = new LinkedHashMap<>();
        List<String> shapes = new ArrayList<>();
        StringBuilder states = new StringBuilder();
        Map<String, String> offsets = new TreeMap<>();
        Method maxH = net.minecraft.world.level.block.state.BlockBehaviour.class.getDeclaredMethod("getMaxHorizontalOffset");
        Method maxV = net.minecraft.world.level.block.state.BlockBehaviour.class.getDeclaredMethod("getMaxVerticalOffset");
        maxH.setAccessible(true);
        maxV.setAccessible(true);
        int count = 0;
        for (BlockState state : Block.BLOCK_STATE_REGISTRY) {
            int id = Block.BLOCK_STATE_REGISTRY.getId(state);
            if (id != count) throw new IllegalStateException("ids out of order at " + id);
            count++;
            Vec3 off = state.getOffset(BlockPos.ZERO);
            VoxelShape shape = state.getShape(EmptyBlockGetter.INSTANCE, BlockPos.ZERO).move(-off.x, -off.y, -off.z);
            StringBuilder edges = new StringBuilder("[");
            shape.forAllEdges((x1, y1, z1, x2, y2, z2) -> {
                if (edges.length() > 1) edges.append(',');
                edges.append(String.format(Locale.ROOT, "[%s,%s,%s,%s,%s,%s]", n(x1), n(y1), n(z1), n(x2), n(y2), n(z2)));
            });
            edges.append(']');
            StringBuilder boxes = new StringBuilder("[");
            for (net.minecraft.world.phys.AABB box : shape.toAabbs()) {
                if (boxes.length() > 1) boxes.append(',');
                boxes.append(String.format(Locale.ROOT, "[%s,%s,%s,%s,%s,%s]", n(box.minX), n(box.minY), n(box.minZ), n(box.maxX), n(box.maxY), n(box.maxZ)));
            }
            boxes.append(']');
            String key = "{\"boxes\":" + boxes + ",\"edges\":" + edges + "}";
            Integer index = shapeIndex.get(key);
            if (index == null) {
                index = shapes.size();
                shapeIndex.put(key, index);
                shapes.add(key);
            }
            if (states.length() > 0) states.append(',');
            states.append(index);
            if (state.hasOffsetFunction()) {
                Block block = state.getBlock();
                String name = BuiltInRegistries.BLOCK.getKey(block).toString();
                boolean xyz = false;
                for (int x = -8; x < 8 && !xyz; x++) for (int z = -8; z < 8; z++) if (state.getOffset(new BlockPos(x, 0, z)).y != 0.0) { xyz = true; break; }
                offsets.put(name, String.format(Locale.ROOT, "[\"%s\",%s,%s]", xyz ? "xyz" : "xz", maxH.invoke(block), maxV.invoke(block)));
            }
        }
        try (Writer w = new OutputStreamWriter(new FileOutputStream(args[0]), "UTF-8")) {
            w.write("{\"minecraft_version\":\"26.3\",\"source\":\"BlockState.getShape(EmptyBlockGetter.INSTANCE, BlockPos.ZERO) less getOffset(BlockPos.ZERO): VoxelShape.toAabbs and forAllEdges\",\"shapes\":[");
            w.write(String.join(",", shapes));
            w.write("],\"states\":[" + states + "],\"offsets\":{");
            boolean first = true;
            for (Map.Entry<String, String> e : offsets.entrySet()) {
                if (!first) w.write(',');
                first = false;
                w.write("\"" + e.getKey() + "\":" + e.getValue());
            }
            w.write("}}");
        }
        System.out.println(count + " states, " + shapes.size() + " shapes, " + offsets.size() + " offset blocks");
    }

    static String n(double v) {
        String s = Double.toString(v);
        return s.endsWith(".0") ? s.substring(0, s.length() - 2) : s;
    }
}
