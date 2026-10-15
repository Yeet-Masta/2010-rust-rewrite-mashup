import java.io.*;
import java.lang.reflect.Field;
import java.lang.reflect.Modifier;
import java.util.*;
import net.minecraft.SharedConstants;
import net.minecraft.client.animation.AnimationChannel;
import net.minecraft.client.animation.AnimationDefinition;
import net.minecraft.client.animation.Keyframe;
import net.minecraft.client.model.geom.LayerDefinitions;
import net.minecraft.client.model.geom.ModelLayerLocation;
import net.minecraft.client.model.geom.ModelPart;
import net.minecraft.client.model.geom.builders.LayerDefinition;
import net.minecraft.server.Bootstrap;
import org.joml.Vector3fc;

// Writes minecraft/data/entity-models-26.3.json (gzip it after). Run against
// the 26.3 client JAR and its libraries (from version.json) on Java 25:
//   javac -proc:none -cp "client.jar:libs/*" DumpModels.java
//   java -cp "client.jar:libs/*:." DumpModels entity-models-26.3.json

/**
 * Every model layer LayerDefinitions.createRoots registers, baked: each part's
 * initial pose and its cubes' polygons (vertex positions in pixels, texture
 * coordinates already over the sheet's size, and the face normal), children
 * by name. Then every keyframe animation in client.animation.definitions:
 * its length, looping, and each bone's channels with their keyframes.
 */
public class DumpModels {
    static Field cubesField, childrenField;

    public static void main(String[] args) throws Exception {
        SharedConstants.tryDetectVersion();
        Bootstrap.bootStrap();
        cubesField = ModelPart.class.getDeclaredField("cubes");
        childrenField = ModelPart.class.getDeclaredField("children");
        cubesField.setAccessible(true);
        childrenField.setAccessible(true);
        Map<ModelLayerLocation, LayerDefinition> roots = LayerDefinitions.createRoots();
        TreeMap<String, String> layers = new TreeMap<>();
        for (Map.Entry<ModelLayerLocation, LayerDefinition> e : roots.entrySet()) {
            ModelPart root = e.getValue().bakeRoot();
            StringBuilder out = new StringBuilder();
            part(out, root);
            layers.put(e.getKey().model().toString() + "#" + e.getKey().layer(), out.toString());
        }
        TreeMap<String, String> animations = new TreeMap<>();
        String[] classes = {"ArmadilloAnimation", "BabyArmadilloAnimation", "BabyAxolotlAnimation", "BabyRabbitAnimation", "BatAnimation", "BreezeAnimation", "CamelAnimation", "CamelBabyAnimation", "CopperGolemAnimation", "CreakingAnimation", "FoxBabyAnimation", "FrogAnimation", "NautilusAnimation", "RabbitAnimation", "SnifferAnimation", "WardenAnimation"};
        for (String name : classes) {
            Class<?> c = Class.forName("net.minecraft.client.animation.definitions." + name);
            for (Field f : c.getDeclaredFields()) {
                if (!Modifier.isStatic(f.getModifiers()) || f.getType() != AnimationDefinition.class) continue;
                f.setAccessible(true);
                animations.put(name + "." + f.getName(), animation((AnimationDefinition) f.get(null)));
            }
        }
        try (Writer w = new BufferedWriter(new OutputStreamWriter(new FileOutputStream(args[0]), "UTF-8"))) {
            w.write("{\"layers\":{");
            boolean first = true;
            for (Map.Entry<String, String> e : layers.entrySet()) {
                if (!first) w.write(',');
                first = false;
                w.write('"' + e.getKey() + "\":" + e.getValue());
            }
            w.write("},\"animations\":{");
            first = true;
            for (Map.Entry<String, String> e : animations.entrySet()) {
                if (!first) w.write(',');
                first = false;
                w.write('"' + e.getKey() + "\":" + e.getValue());
            }
            w.write("}}");
        }
        System.out.println(layers.size() + " layers, " + animations.size() + " animations");
    }

    /** {"p":[x,y,z,xRot,yRot,zRot,xScale,yScale,zScale],"q":[polygons...],"c":{name:part}} with each polygon 4 x (x,y,z,u,v) then the normal. */
    @SuppressWarnings("unchecked")
    static void part(StringBuilder out, ModelPart p) throws Exception {
        out.append("{\"p\":[").append(n(p.x)).append(',').append(n(p.y)).append(',').append(n(p.z)).append(',')
            .append(n(p.xRot)).append(',').append(n(p.yRot)).append(',').append(n(p.zRot)).append(',')
            .append(n(p.xScale)).append(',').append(n(p.yScale)).append(',').append(n(p.zScale)).append(']');
        List<ModelPart.Cube> cubes = (List<ModelPart.Cube>) cubesField.get(p);
        if (!cubes.isEmpty()) {
            out.append(",\"q\":[");
            boolean first = true;
            for (ModelPart.Cube cube : cubes) {
                for (ModelPart.Polygon polygon : cube.polygons) {
                    if (polygon.vertices().length != 4) throw new IllegalStateException("non-quad polygon");
                    for (ModelPart.Vertex v : polygon.vertices()) {
                        if (!first) out.append(',');
                        first = false;
                        out.append(n(v.x())).append(',').append(n(v.y())).append(',').append(n(v.z())).append(',').append(n(v.u())).append(',').append(n(v.v()));
                    }
                    Vector3fc normal = polygon.normal();
                    out.append(',').append(n(normal.x())).append(',').append(n(normal.y())).append(',').append(n(normal.z()));
                }
            }
            out.append(']');
        }
        Map<String, ModelPart> children = new TreeMap<>((Map<String, ModelPart>) childrenField.get(p));
        if (!children.isEmpty()) {
            out.append(",\"c\":{");
            boolean first = true;
            for (Map.Entry<String, ModelPart> e : children.entrySet()) {
                if (!first) out.append(',');
                first = false;
                out.append('"').append(e.getKey()).append("\":");
                part(out, e.getValue());
            }
            out.append('}');
        }
        out.append('}');
    }

    /** {"len":seconds,"loop":bool,"bones":{bone:[[target,[t,preX,preY,preZ,postX,postY,postZ,catmullrom],...],...]}} with target 0 position, 1 rotation, 2 scale. */
    static String animation(AnimationDefinition a) {
        StringBuilder out = new StringBuilder("{\"len\":").append(n(a.lengthInSeconds())).append(",\"loop\":").append(a.looping()).append(",\"bones\":{");
        boolean firstBone = true;
        for (Map.Entry<String, List<AnimationChannel>> bone : new TreeMap<>(a.boneAnimations()).entrySet()) {
            if (!firstBone) out.append(',');
            firstBone = false;
            out.append('"').append(bone.getKey()).append("\":[");
            boolean firstChannel = true;
            for (AnimationChannel channel : bone.getValue()) {
                if (!firstChannel) out.append(',');
                firstChannel = false;
                int target = channel.target() == AnimationChannel.Targets.POSITION ? 0 : channel.target() == AnimationChannel.Targets.ROTATION ? 1 : channel.target() == AnimationChannel.Targets.SCALE ? 2 : -1;
                if (target < 0) throw new IllegalStateException("unknown target");
                out.append('[').append(target);
                for (Keyframe k : channel.keyframes()) {
                    boolean catmull = k.interpolation() == AnimationChannel.Interpolations.CATMULLROM;
                    if (!catmull && k.interpolation() != AnimationChannel.Interpolations.LINEAR) throw new IllegalStateException("unknown interpolation");
                    out.append(",[").append(n(k.timestamp())).append(',')
                        .append(n(k.preTarget().x())).append(',').append(n(k.preTarget().y())).append(',').append(n(k.preTarget().z())).append(',')
                        .append(n(k.postTarget().x())).append(',').append(n(k.postTarget().y())).append(',').append(n(k.postTarget().z())).append(',')
                        .append(catmull ? 1 : 0).append(']');
                }
                out.append(']');
            }
            out.append(']');
        }
        return out.append("}}").toString();
    }

    /** The float's shortest decimal that reads back as the same float. */
    static String n(float f) {
        // The boat's water patch has no sheet: its coordinates divide by zero.
        if (Float.isNaN(f) || Float.isInfinite(f)) return "0";
        if (f == (long) f && Math.abs(f) < 1e9) return Long.toString((long) f);
        return Float.toString(f);
    }
}
