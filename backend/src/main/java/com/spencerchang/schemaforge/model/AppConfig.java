package com.spencerchang.schemaforge.model;

import com.fasterxml.jackson.annotation.JsonIgnoreProperties;
import com.fasterxml.jackson.annotation.JsonInclude;
import com.fasterxml.jackson.annotation.JsonProperty;

import java.util.ArrayList;
import java.util.List;

@JsonInclude(JsonInclude.Include.NON_EMPTY)
@JsonIgnoreProperties(ignoreUnknown = true)
public class AppConfig {

    private Spring spring = new Spring();
    private Screw screw = new Screw();

    public Spring getSpring() {
        return spring;
    }

    public void setSpring(Spring spring) {
        this.spring = spring;
    }

    public Screw getScrew() {
        return screw;
    }

    public void setScrew(Screw screw) {
        this.screw = screw;
    }

    @JsonIgnoreProperties(ignoreUnknown = true)
    @JsonInclude(JsonInclude.Include.NON_EMPTY)
    public static class Spring {
        private DataSource datasource = new DataSource();

        public DataSource getDatasource() {
            return datasource;
        }

        public void setDatasource(DataSource datasource) {
            this.datasource = datasource;
        }
    }

    @JsonIgnoreProperties(ignoreUnknown = true)
    @JsonInclude(JsonInclude.Include.NON_EMPTY)
    public static class DataSource {
        @JsonProperty("driver-class-name")
        private String driverClassName = "com.mysql.cj.jdbc.Driver";
        private String url = "jdbc:mysql://127.0.0.1:3306/?useUnicode=true&characterEncoding=utf8&useSSL=false&serverTimezone=Asia/Shanghai";
        private String username = "your_username";
        private String password = "your_password";

        public String getDriverClassName() {
            return driverClassName;
        }

        public void setDriverClassName(String driverClassName) {
            this.driverClassName = driverClassName;
        }

        public String getUrl() {
            return url;
        }

        public void setUrl(String url) {
            this.url = url;
        }

        public String getUsername() {
            return username;
        }

        public void setUsername(String username) {
            this.username = username;
        }

        public String getPassword() {
            return password;
        }

        public void setPassword(String password) {
            this.password = password;
        }
    }

    @JsonIgnoreProperties(ignoreUnknown = true)
    @JsonInclude(JsonInclude.Include.NON_EMPTY)
    public static class Screw {
        private List<String> schemas = new ArrayList<>();
        private Engine engine = new Engine();

        public Screw() {
            schemas.add("your_database");
        }

        public List<String> getSchemas() {
            return schemas;
        }

        public void setSchemas(List<String> schemas) {
            this.schemas = schemas;
        }

        public Engine getEngine() {
            return engine;
        }

        public void setEngine(Engine engine) {
            this.engine = engine;
        }
    }

    @JsonIgnoreProperties(ignoreUnknown = true)
    @JsonInclude(JsonInclude.Include.NON_EMPTY)
    public static class Engine {
        @JsonProperty("file-output-dir")
        private String fileOutputDir = System.getProperty("user.home") + "/Downloads/";
        @JsonProperty("open-output-dir")
        private Boolean openOutputDir = true;
        @JsonProperty("file-type")
        private String fileType = "HTML";
        @JsonProperty("produce-type")
        private String produceType = "freemarker";
        @JsonProperty("file-name")
        private String fileName;

        public String getFileOutputDir() {
            return fileOutputDir;
        }

        public void setFileOutputDir(String fileOutputDir) {
            this.fileOutputDir = fileOutputDir;
        }

        public Boolean getOpenOutputDir() {
            return openOutputDir;
        }

        public void setOpenOutputDir(Boolean openOutputDir) {
            this.openOutputDir = openOutputDir;
        }

        public String getFileType() {
            return fileType;
        }

        public void setFileType(String fileType) {
            this.fileType = fileType;
        }

        public String getProduceType() {
            return produceType;
        }

        public void setProduceType(String produceType) {
            this.produceType = produceType;
        }

        public String getFileName() {
            return fileName;
        }

        public void setFileName(String fileName) {
            this.fileName = fileName;
        }
    }
}
